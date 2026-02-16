use crate::domain::{DomainError, MediaCounts, MediaItem, MediaSummary};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use super::{load_tags_bulk, load_tags_for_media, SqliteRepository};

impl SqliteRepository {
    pub(crate) fn save_metadata_and_vector_impl(
        &self,
        media: &MediaItem,
        vector: Option<&[f32]>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let uuid_bytes = media.id.as_bytes();
            let timestamp_str = media.uploaded_at.to_rfc3339();
            let original_date_str = media.original_date.to_rfc3339();

            let res = conn.execute(
                "INSERT INTO media (id, filename, original_filename, media_type, phash, uploaded_at, original_date, width, height, size_bytes, exif_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    uuid_bytes,
                    media.filename,
                    media.original_filename,
                    media.media_type,
                    media.phash,
                    timestamp_str,
                    original_date_str,
                    media.width,
                    media.height,
                    media.size_bytes,
                    media.exif_json
                ],
            );

            if let Err(e) = res {
                let _ = conn.execute("ROLLBACK", []);
                return Err(DomainError::Database(e.to_string()));
            }

            if let Some(v) = vector {
                let vector_bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        v.as_ptr() as *const u8,
                        v.len() * size_of::<f32>(),
                    )
                };

                let media_rowid = conn.last_insert_rowid();

                let res = conn.execute(
                    "INSERT INTO vec_media (rowid, embedding) VALUES (?1, ?2)",
                    params![media_rowid, vector_bytes],
                );

                if let Err(e) = res {
                    let _ = conn.execute("ROLLBACK", []);
                    return Err(DomainError::Database(e.to_string()));
                }
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn exists_by_phash_impl(&self, phash: &str) -> Result<bool, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT count(*) FROM media WHERE phash = ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let count: i64 = stmt
                .query_row([phash], |row| row.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;

            Ok(count > 0)
        })
    }

    pub(crate) fn find_similar_impl(
        &self,
        vector: &[f32],
        limit: usize,
        max_distance: f32,
    ) -> Result<Vec<MediaItem>, DomainError> {
        self.with_conn(|conn| {
            let vector_bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    vector.as_ptr() as *const u8,
                    vector.len() * size_of::<f32>(),
                )
            };

            let mut stmt = conn.prepare(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, v.distance, (f.media_id IS NOT NULL) as is_favorite
                 FROM (
                    SELECT rowid, distance
                    FROM vec_media
                    WHERE embedding MATCH ?1
                    ORDER BY distance
                    LIMIT ?2
                 ) v
                 JOIN media m ON m.rowid = v.rowid
                 LEFT JOIN favorites f ON f.media_id = m.id
                 WHERE v.distance <= ?3
                 ORDER BY v.distance"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(
                    params![vector_bytes, limit as i64, max_distance],
                    |row| {
                        let id_bytes: Vec<u8> = row.get(0)?;
                        let filename: String = row.get(1)?;
                        let original_filename: String = row.get(2)?;
                        let media_type: String = row.get(3)?;
                        let phash: String = row.get(4)?;
                        let timestamp_str: String = row.get(5)?;
                        let original_date_str: String = row.get(6)?;
                        let width: Option<u32> = row.get(7)?;
                        let height: Option<u32> = row.get(8)?;
                        let size_bytes: i64 = row.get(9)?;
                        let exif_json: Option<String> = row.get(10)?;
                        let is_favorite: bool = row.get(12)?;

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
                                    5,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                )
                            })?
                            .with_timezone(&Utc);
                        let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                            .map_err(|e| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    6,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                )
                            })?
                            .with_timezone(&Utc);

                        Ok((
                            id_bytes,
                            MediaItem {
                                id,
                                filename,
                                original_filename,
                                media_type,
                                phash,
                                uploaded_at,
                                original_date,
                                width,
                                height,
                                size_bytes,
                                exif_json,
                                is_favorite,
                                tags: vec![],
                            },
                        ))
                    },
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut items_with_ids: Vec<(Vec<u8>, MediaItem)> = Vec::new();
            for row in rows {
                items_with_ids
                    .push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }

            // Load tags in bulk
            let id_bytes_list: Vec<Vec<u8>> =
                items_with_ids.iter().map(|(id, _)| id.clone()).collect();
            let tags_map = load_tags_bulk(conn, &id_bytes_list);

            let items = items_with_ids
                .into_iter()
                .map(|(id_bytes, mut item)| {
                    if let Some(tags) = tags_map.get(&id_bytes) {
                        item.tags = tags.clone();
                    }
                    item
                })
                .collect();

            Ok(items)
        })
    }

    pub(crate) fn find_by_id_impl(&self, id: Uuid) -> Result<Option<MediaItem>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, (f.media_id IS NOT NULL) as is_favorite
                 FROM media m
                 LEFT JOIN favorites f ON f.media_id = m.id
                 WHERE m.id = ?1"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let result = stmt.query_row(params![id.as_bytes()], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let phash: String = row.get(4)?;
                let timestamp_str: String = row.get(5)?;
                let original_date_str: String = row.get(6)?;
                let width: Option<u32> = row.get(7)?;
                let height: Option<u32> = row.get(8)?;
                let size_bytes: i64 = row.get(9)?;
                let exif_json: Option<String> = row.get(10)?;
                let is_favorite: bool = row.get(11)?;

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
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?
                    .with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?
                    .with_timezone(&Utc);

                Ok(MediaItem {
                    id,
                    filename,
                    original_filename,
                    media_type,
                    phash,
                    uploaded_at,
                    original_date,
                    width,
                    height,
                    size_bytes,
                    exif_json,
                    is_favorite,
                    tags: vec![],
                })
            });

            match result {
                Ok(mut item) => {
                    item.tags = load_tags_for_media(conn, id.as_bytes());
                    Ok(Some(item))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    pub(crate) fn delete_impl(&self, id: Uuid) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rowid: Option<i64> = conn
                .prepare("SELECT rowid FROM media WHERE id = ?1")
                .and_then(|mut s| s.query_row(params![id.as_bytes()], |r| r.get(0)))
                .ok();

            // Clean up tags
            let _ = conn.execute(
                "DELETE FROM media_tags WHERE media_id = ?1",
                params![id.as_bytes()],
            );

            // Clean up favorites
            let _ = conn.execute(
                "DELETE FROM favorites WHERE media_id = ?1",
                params![id.as_bytes()],
            );

            let deleted = conn
                .execute("DELETE FROM media WHERE id = ?1", params![id.as_bytes()])
                .map_err(|e| {
                    let _ = conn.execute("ROLLBACK", []);
                    DomainError::Database(e.to_string())
                })?;

            if deleted == 0 {
                let _ = conn.execute("ROLLBACK", []);
                return Err(DomainError::NotFound);
            }

            if let Some(rowid) = rowid {
                let _ = conn.execute("DELETE FROM vec_media WHERE rowid = ?1", params![rowid]);
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_embedding_impl(&self, id: Uuid) -> Result<Option<Vec<f32>>, DomainError> {
        self.with_conn(|conn| {
            let rowid: Option<i64> = conn
                .prepare("SELECT rowid FROM media WHERE id = ?1")
                .and_then(|mut s| s.query_row(params![id.as_bytes()], |r| r.get(0)))
                .ok();

            let rowid = match rowid {
                Some(r) => r,
                None => return Ok(None),
            };

            let mut stmt = conn
                .prepare("SELECT embedding FROM vec_media WHERE rowid = ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let embedding_bytes: Option<Vec<u8>> =
                stmt.query_row(params![rowid], |row| row.get(0)).ok();

            match embedding_bytes {
                Some(bytes) => {
                    if bytes.len() % 4 != 0 {
                        return Err(DomainError::Database(
                            "Invalid embedding length".to_string(),
                        ));
                    }
                    let count = bytes.len() / 4;
                    let mut floats = Vec::with_capacity(count);
                    for chunk in bytes.chunks_exact(4) {
                        let arr: [u8; 4] = chunk
                            .try_into()
                            .map_err(|_| DomainError::Database("Conversion error".to_string()))?;
                        floats.push(f32::from_ne_bytes(arr));
                    }
                    super::normalize_vector(&mut floats);
                    Ok(Some(floats))
                }
                None => Ok(None),
            }
        })
    }

    pub(crate) fn delete_many_impl(&self, ids: &[Uuid]) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut deleted = 0usize;
            for id in ids {
                let rowid: Option<i64> = conn
                    .prepare("SELECT rowid FROM media WHERE id = ?1")
                    .and_then(|mut s| s.query_row(params![id.as_bytes()], |r| r.get(0)))
                    .ok();

                // Clean up tags
                let _ = conn.execute(
                    "DELETE FROM media_tags WHERE media_id = ?1",
                    params![id.as_bytes()],
                );

                // Clean up favorites
                let _ = conn.execute(
                    "DELETE FROM favorites WHERE media_id = ?1",
                    params![id.as_bytes()],
                );

                let count = conn
                    .execute("DELETE FROM media WHERE id = ?1", params![id.as_bytes()])
                    .map_err(|e| {
                        let _ = conn.execute("ROLLBACK", []);
                        DomainError::Database(e.to_string())
                    })?;

                if let Some(rowid) = rowid {
                    let _ = conn.execute("DELETE FROM vec_media WHERE rowid = ?1", params![rowid]);
                }

                deleted += count;
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(deleted)
        })
    }

    pub(crate) fn find_all_impl(
        &self,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        tags: Option<Vec<String>>,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let order = if sort_asc { "ASC" } else { "DESC" };

            let mut sql = "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (f.media_id IS NOT NULL) as is_favorite, m.size_bytes
                         FROM media m
                         LEFT JOIN favorites f ON f.media_id = m.id".to_string();

            let mut conditions = Vec::new();
            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(mt) = media_type {
                conditions.push("m.media_type = ?".to_string());
                params_vec.push(Box::new(mt.to_string()));
            }

            if favorite {
                conditions.push("f.media_id IS NOT NULL".to_string());
            }

            // Tag filtering: media must have ALL specified tags
            if let Some(ref tag_list) = tags {
                if !tag_list.is_empty() {
                    for tag in tag_list {
                        conditions.push(
                            "EXISTS (SELECT 1 FROM media_tags mt2 JOIN tags t2 ON t2.id = mt2.tag_id WHERE mt2.media_id = m.id AND t2.name = ?)".to_string(),
                        );
                        params_vec.push(Box::new(tag.clone()));
                    }
                }
            }

            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&conditions.join(" AND "));
            }

            sql.push_str(&format!(
                " ORDER BY m.original_date {} LIMIT ? OFFSET ?",
                order
            ));
            params_vec.push(Box::new(limit as i64));
            params_vec.push(Box::new(offset as i64));

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
                    let is_favorite: bool = row.get(6)?;
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

                    Ok((
                        id_bytes,
                        MediaSummary {
                            id,
                            filename,
                            original_filename,
                            media_type,
                            uploaded_at,
                            original_date,
                            size_bytes,
                            is_favorite,
                            tags: vec![],
                        },
                    ))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut items_with_ids: Vec<(Vec<u8>, MediaSummary)> = Vec::new();
            for row in rows {
                items_with_ids
                    .push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }

            // Load tags in bulk
            let id_bytes_list: Vec<Vec<u8>> =
                items_with_ids.iter().map(|(id, _)| id.clone()).collect();
            let tags_map = load_tags_bulk(conn, &id_bytes_list);

            let items = items_with_ids
                .into_iter()
                .map(|(id_bytes, mut summary)| {
                    if let Some(tags) = tags_map.get(&id_bytes) {
                        summary.tags = tags.clone();
                    }
                    summary
                })
                .collect();

            Ok(items)
        })
    }

    pub(crate) fn media_counts_impl(&self) -> Result<MediaCounts, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT
                    COUNT(*) AS total,
                    COALESCE(SUM(CASE WHEN media_type = 'image' THEN 1 ELSE 0 END), 0) AS images,
                    COALESCE(SUM(CASE WHEN media_type = 'video' THEN 1 ELSE 0 END), 0) AS videos,
                    COALESCE(SUM(size_bytes), 0) AS total_size_bytes
                 FROM media",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            stmt.query_row([], |row| {
                Ok(MediaCounts {
                    total: row.get(0)?,
                    images: row.get(1)?,
                    videos: row.get(2)?,
                    total_size_bytes: row.get(3)?,
                })
            })
            .map_err(|e| DomainError::Database(e.to_string()))
        })
    }

    pub(crate) fn set_favorite_impl(&self, id: Uuid, favorite: bool) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            if favorite {
                conn.execute(
                    "INSERT OR IGNORE INTO favorites (media_id, created_at) VALUES (?1, ?2)",
                    params![id.as_bytes(), Utc::now().to_rfc3339()],
                )
            } else {
                conn.execute(
                    "DELETE FROM favorites WHERE media_id = ?1",
                    params![id.as_bytes()],
                )
            }
            .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }
}
