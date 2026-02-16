use crate::domain::{DomainError, MediaSummary};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use super::SqliteRepository;

impl SqliteRepository {
    pub(crate) fn get_all_embeddings_impl(
        &self,
        folder_id: Option<Uuid>,
    ) -> Result<Vec<(MediaSummary, Vec<f32>)>, DomainError> {
        self.with_conn(|conn| {
            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                match folder_id {
                    Some(fid) => (
                        "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, v.embedding, m.size_bytes
                     FROM media m
                     JOIN folder_media fm ON fm.media_id = m.id
                     JOIN vec_media v ON v.rowid = m.rowid
                     WHERE fm.folder_id = ?1"
                            .to_string(),
                        vec![
                            Box::new(fid.as_bytes().to_vec()) as Box<dyn rusqlite::types::ToSql>
                        ],
                    ),
                    None => (
                        "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, v.embedding, m.size_bytes
                     FROM media m
                     JOIN vec_media v ON v.rowid = m.rowid"
                            .to_string(),
                        vec![],
                    ),
                };

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let filename: String = row.get(1)?;
                    let original_filename: String = row.get(2)?;
                    let media_type: String = row.get(3)?;
                    let timestamp_str: String = row.get(4)?;
                    let original_date_str: String = row.get(5)?;
                    let embedding_bytes: Vec<u8> = row.get(6)?;
                    let size_bytes: i64 = row.get(7)?;

                    let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            Box::new(e),
                        )
                    })?;
                    let uploaded_at = DateTime::parse_from_rfc3339(&timestamp_str)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                4,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .with_timezone(&Utc);
                    let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .with_timezone(&Utc);

                    let summary = MediaSummary {
                        id,
                        filename,
                        original_filename,
                        media_type,
                        uploaded_at,
                        original_date,
                        size_bytes,
                        is_favorite: false,
                        tags: vec![],
                    };

                    // Parse embedding bytes into f32 vec
                    if embedding_bytes.len() % 4 != 0 {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Blob,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid embedding length",
                            )),
                        ));
                    }
                    let mut vector = Vec::with_capacity(embedding_bytes.len() / 4);
                    for chunk in embedding_bytes.chunks_exact(4) {
                        let arr: [u8; 4] = chunk.try_into().unwrap();
                        vector.push(f32::from_ne_bytes(arr));
                    }

                    super::normalize_vector(&mut vector);

                    Ok((summary, vector))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn get_random_embeddings_impl(
        &self,
        limit: usize,
        exclude_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Vec<f32>)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT m.id, v.embedding FROM media m
                 JOIN vec_media v ON v.rowid = m.rowid
                 ORDER BY RANDOM() LIMIT ?1",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let embedding_bytes: Vec<u8> = row.get(1)?;
                    let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            Box::new(e),
                        )
                    })?;

                    let mut vector = Vec::with_capacity(embedding_bytes.len() / 4);
                    for chunk in embedding_bytes.chunks_exact(4) {
                        let arr: [u8; 4] = chunk.try_into().unwrap();
                        vector.push(f32::from_ne_bytes(arr));
                    }
                    super::normalize_vector(&mut vector);
                    Ok((id, vector))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                let (id, vector) = row.map_err(|e| DomainError::Database(e.to_string()))?;
                if !exclude_ids.contains(&id) {
                    results.push((id, vector));
                }
            }
            Ok(results)
        })
    }

    pub(crate) fn get_nearest_embeddings_impl(
        &self,
        vector: &[f32],
        limit: usize,
        exclude_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, Vec<f32>)>, DomainError> {
        self.with_conn(|conn| {
            let vector_bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(vector.as_ptr() as *const u8, vector.len() * 4)
            };

            let mut stmt = conn
                .prepare(
                    "SELECT m.id, v2.embedding
                     FROM (
                         SELECT rowid, distance FROM vec_media
                         WHERE embedding MATCH ?1
                         ORDER BY distance
                         LIMIT ?2
                     ) v
                     JOIN media m ON m.rowid = v.rowid
                     JOIN vec_media v2 ON v2.rowid = v.rowid",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Fetch extra to account for potential exclusions
            let fetch_limit = limit + exclude_ids.len();
            let rows = stmt
                .query_map(params![vector_bytes, fetch_limit as i64], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let embedding_bytes: Vec<u8> = row.get(1)?;
                    let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            Box::new(e),
                        )
                    })?;

                    let mut vector = Vec::with_capacity(embedding_bytes.len() / 4);
                    for chunk in embedding_bytes.chunks_exact(4) {
                        let arr: [u8; 4] = chunk.try_into().unwrap();
                        vector.push(f32::from_ne_bytes(arr));
                    }
                    super::normalize_vector(&mut vector);
                    Ok((id, vector))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                let (id, vector) = row.map_err(|e| DomainError::Database(e.to_string()))?;
                if !exclude_ids.contains(&id) {
                    results.push((id, vector));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
            Ok(results)
        })
    }
}
