use crate::domain::{DomainError, MediaSummary};
use chrono::{DateTime, Utc};
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
                        "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, v.embedding
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
                        "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, v.embedding
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

                    // L2-normalize in-place so cosine distance = 1 - dot(a, b)
                    let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
                    if norm > 0.0 {
                        let inv = 1.0 / norm;
                        for v in vector.iter_mut() {
                            *v *= inv;
                        }
                    }

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
}
