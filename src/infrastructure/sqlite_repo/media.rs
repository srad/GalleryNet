use crate::domain::{DomainError, MediaCounts, MediaItem, MediaSummary};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use super::faces::load_faces_for_media;
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

    pub(crate) fn update_media_and_vector_impl(
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

            // Update media table
            let res = conn.execute(
                "UPDATE media SET 
                    filename = ?2,
                    original_filename = ?3,
                    media_type = ?4,
                    phash = ?5,
                    uploaded_at = ?6,
                    original_date = ?7,
                    width = ?8,
                    height = ?9,
                    size_bytes = ?10,
                    exif_json = ?11
                 WHERE id = ?1",
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

            // Update vector: get rowid, delete from vec_media, re-insert
            let rowid: Option<i64> = conn
                .prepare("SELECT rowid FROM media WHERE id = ?1")
                .and_then(|mut s| s.query_row(params![uuid_bytes], |r| r.get(0)))
                .ok();

            if let Some(rowid) = rowid {
                let _ = conn.execute("DELETE FROM vec_media WHERE rowid = ?1", params![rowid]);

                if let Some(v) = vector {
                    let vector_bytes: &[u8] = unsafe {
                        std::slice::from_raw_parts(
                            v.as_ptr() as *const u8,
                            v.len() * size_of::<f32>(),
                        )
                    };

                    let res = conn.execute(
                        "INSERT INTO vec_media (rowid, embedding) VALUES (?1, ?2)",
                        params![rowid, vector_bytes],
                    );

                    if let Err(e) = res {
                        let _ = conn.execute("ROLLBACK", []);
                        return Err(DomainError::Database(e.to_string()));
                    }
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
                                faces_scanned: false, // Default or load from DB if we add it to the SELECT
                                tags: vec![],
                                faces: vec![],
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
                    faces_scanned: false, // Default or load from DB
                    tags: vec![],
                    faces: vec![],
                })
            });

            match result {
                Ok(mut item) => {
                    item.tags = load_tags_for_media(conn, id.as_bytes());
                    item.faces = load_faces_for_media(conn, id.as_bytes());
                    Ok(Some(item))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    pub(crate) fn find_media_without_phash_impl(&self) -> Result<Vec<MediaItem>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, (f.media_id IS NOT NULL) as is_favorite
                 FROM media m
                 LEFT JOIN favorites f ON f.media_id = m.id
                 WHERE m.phash = 'no_hash'"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
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
                    faces_scanned: false,
                    tags: vec![],
                    faces: vec![],
                })


                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(items)
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
        person_id: Option<Uuid>,
        cluster_id: Option<i64>,
        sort_asc: bool,
        sort_by: &str,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let order = if sort_asc { "ASC" } else { "DESC" };
            let order_column = match sort_by {
                "size" => "m.size_bytes",
                _ => "m.original_date",
            };

            let mut sql = "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (f.media_id IS NOT NULL) as is_favorite, m.size_bytes, m.width, m.height
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

            if let Some(pid) = person_id {
                conditions.push("EXISTS (SELECT 1 FROM faces fs WHERE fs.media_id = m.id AND fs.person_id = ?)".to_string());
                params_vec.push(Box::new(pid.as_bytes().to_vec()));
            }

            if let Some(cid) = cluster_id {
                conditions.push("EXISTS (SELECT 1 FROM faces fs WHERE fs.media_id = m.id AND fs.cluster_id = ?)".to_string());
                params_vec.push(Box::new(cid));
            }

            // Tag filtering: media must have ANY of the specified tags (OR)
            if let Some(ref tag_list) = tags {

                if !tag_list.is_empty() {
                    let placeholders: Vec<String> = tag_list.iter().map(|_| "?".to_string()).collect();
                    conditions.push(format!(
                        "EXISTS (SELECT 1 FROM media_tags mt2 JOIN tags t2 ON t2.id = mt2.tag_id WHERE mt2.media_id = m.id AND t2.name IN ({}))",
                        placeholders.join(", ")
                    ));
                    for tag in tag_list {
                        params_vec.push(Box::new(tag.clone()));
                    }
                }
            }

            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&conditions.join(" AND "));
            }

            sql.push_str(&format!(
                " ORDER BY {} {} LIMIT ? OFFSET ?",
                order_column, order
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
                    let width: Option<u32> = row.get(8)?;
                    let height: Option<u32> = row.get(9)?;

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
                            width,
                            height,
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

#[cfg(test)]
mod tests {
    use super::super::TestDb;
    use crate::infrastructure::SqliteRepository;
    use rusqlite::params;
    use uuid::Uuid;

    /// Insert a media row with a given original_date and size_bytes.
    fn insert_media(repo: &SqliteRepository, id: Uuid, date: &str, size: i64) {
        repo.with_conn(|conn| {
            conn.execute(
                "INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date)
                 VALUES (?1, ?2, ?3, ?4, 'ph', '2024-01-01T00:00:00Z', ?5)",
                params![
                    id.as_bytes(),
                    format!("{}.jpg", id),
                    format!("{}.jpg", id),
                    size,
                    date,
                ],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_sort_by_date_desc() {
        let db = TestDb::new("test_sort_date_desc");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 200);
        insert_media(&db.repo, id3, "2024-03-10T00:00:00Z", 300);

        let results = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(results.len(), 3);

        // DESC by date: Jun, Mar, Jan
        assert_eq!(results[0].id, id2);
        assert_eq!(results[1].id, id3);
        assert_eq!(results[2].id, id1);
    }

    #[test]
    fn test_sort_by_date_asc() {
        let db = TestDb::new("test_sort_date_asc");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 200);
        insert_media(&db.repo, id3, "2024-03-10T00:00:00Z", 300);

        let results = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, true, "date")
            .unwrap();
        assert_eq!(results.len(), 3);

        // ASC by date: Jan, Mar, Jun
        assert_eq!(results[0].id, id1);
        assert_eq!(results[1].id, id3);
        assert_eq!(results[2].id, id2);
    }

    #[test]
    fn test_sort_by_size_desc() {
        let db = TestDb::new("test_sort_size_desc");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 500);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 100);
        insert_media(&db.repo, id3, "2024-03-10T00:00:00Z", 9999);

        let results = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, false, "size")
            .unwrap();

        assert_eq!(results.len(), 3);
        // DESC by size: 9999, 500, 100
        assert_eq!(results[0].id, id3);
        assert_eq!(results[1].id, id1);
        assert_eq!(results[2].id, id2);
    }

    #[test]
    fn test_sort_by_size_asc() {
        let db = TestDb::new("test_sort_size_asc");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 500);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 100);
        insert_media(&db.repo, id3, "2024-03-10T00:00:00Z", 9999);

        let results = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, true, "size")
            .unwrap();
        assert_eq!(results.len(), 3);
        // ASC by size: 100, 500, 9999
        assert_eq!(results[0].id, id2);
        assert_eq!(results[1].id, id1);
        assert_eq!(results[2].id, id3);
    }

    #[test]
    fn test_sort_by_unknown_field_defaults_to_date() {
        let db = TestDb::new("test_sort_unknown");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 50);

        // Unknown sort_by value should default to date ordering
        let results = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                None,
                None,
                None,
                false,
                "bogus; DROP TABLE media;--",
            )
            .unwrap();
        assert_eq!(results.len(), 2);
        // DESC by date: Jun, Jan
        assert_eq!(results[0].id, id2);
        assert_eq!(results[1].id, id1);
    }

    #[test]
    fn test_sort_by_size_in_folder() {
        let db = TestDb::new("test_sort_folder_size");

        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 500);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 100);
        insert_media(&db.repo, id3, "2024-03-10T00:00:00Z", 9999);

        // Create folder and add media
        db.repo
            .create_folder_impl(folder_id, "Test Folder")
            .unwrap();
        db.repo
            .add_media_to_folder_impl(folder_id, &[id1, id2, id3])
            .unwrap();

        // Sort folder by size descending
        let results = db
            .repo
            .find_all_in_folder_impl(
                folder_id, 10, 0, None, false, None, None, None, false, "size",
            )
            .unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, id3); // 9999
        assert_eq!(results[1].id, id1); // 500
        assert_eq!(results[2].id, id2); // 100

        // Sort folder by size ascending
        let results = db
            .repo
            .find_all_in_folder_impl(
                folder_id, 10, 0, None, false, None, None, None, true, "size",
            )
            .unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, id2); // 100
        assert_eq!(results[1].id, id1); // 500
        assert_eq!(results[2].id, id3); // 9999
    }

    // ==================== Filtering tests ====================

    #[test]
    fn test_filter_by_media_type() {
        let db = TestDb::new("test_filter_media_type");

        let img1 = Uuid::new_v4();
        let img2 = Uuid::new_v4();
        let vid1 = Uuid::new_v4();
        insert_media(&db.repo, img1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, img2, "2024-02-01T00:00:00Z", 200);
        insert_media(&db.repo, vid1, "2024-03-01T00:00:00Z", 300);

        // Mark vid1 as video
        db.repo
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE media SET media_type = 'video' WHERE id = ?1",
                    params![vid1.as_bytes()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();

        // Filter images only
        let images = db
            .repo
            .find_all_impl(10, 0, Some("image"), false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(images.len(), 2);
        assert!(images.iter().all(|m| m.media_type == "image"));

        // Filter videos only
        let videos = db
            .repo
            .find_all_impl(10, 0, Some("video"), false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, vid1);

        // No filter — returns all
        let all = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_filter_by_favorite() {
        let db = TestDb::new("test_filter_favorite");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-02-01T00:00:00Z", 200);
        insert_media(&db.repo, id3, "2024-03-01T00:00:00Z", 300);

        // Favorite id1 and id3
        db.repo.set_favorite_impl(id1, true).unwrap();
        db.repo.set_favorite_impl(id3, true).unwrap();

        // Filter favorites
        let favs = db
            .repo
            .find_all_impl(10, 0, None, true, None, None, None, false, "date")
            .unwrap();
        assert_eq!(favs.len(), 2);
        assert!(favs.iter().all(|m| m.is_favorite));

        // Unfavorite id1
        db.repo.set_favorite_impl(id1, false).unwrap();
        let favs = db
            .repo
            .find_all_impl(10, 0, None, true, None, None, None, false, "date")
            .unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, id3);

        // No favorite filter — all returned, with correct is_favorite flag
        let all = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(all.len(), 3);
        // id3 (Mar) is first in desc order and is favorited
        assert!(all.iter().find(|m| m.id == id3).unwrap().is_favorite);
        assert!(!all.iter().find(|m| m.id == id1).unwrap().is_favorite);
    }

    #[test]
    fn test_filter_by_tags() {
        let db = TestDb::new("test_filter_tags");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-02-01T00:00:00Z", 200);
        insert_media(&db.repo, id3, "2024-03-01T00:00:00Z", 300);

        // Tag id1 with "Nature", id2 with "City", id3 with both
        db.repo
            .update_media_tags_impl(id1, vec!["Nature".to_string()])
            .unwrap();
        db.repo
            .update_media_tags_impl(id2, vec!["City".to_string()])
            .unwrap();
        db.repo
            .update_media_tags_impl(id3, vec!["Nature".to_string(), "City".to_string()])
            .unwrap();

        // Filter by "Nature" — should return id1 and id3
        let nature = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                Some(vec!["Nature".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(nature.len(), 2);
        let nature_ids: Vec<Uuid> = nature.iter().map(|m| m.id).collect();
        assert!(nature_ids.contains(&id1));
        assert!(nature_ids.contains(&id3));

        // Filter by "City" — should return id2 and id3
        let city = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                Some(vec!["City".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(city.len(), 2);

        // Filter by both tags (OR) — should return all 3
        let both = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                Some(vec!["Nature".to_string(), "City".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(both.len(), 3);

        // Filter by nonexistent tag — should return nothing
        let none = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                Some(vec!["Nonexistent".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(none.len(), 0);
    }

    #[test]
    fn test_combined_filters() {
        let db = TestDb::new("test_combined_filters");

        let id1 = Uuid::new_v4(); // image, fav, Nature
        let id2 = Uuid::new_v4(); // video, fav, Nature
        let id3 = Uuid::new_v4(); // image, not fav, City
        let id4 = Uuid::new_v4(); // image, fav, City
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-02-01T00:00:00Z", 200);
        insert_media(&db.repo, id3, "2024-03-01T00:00:00Z", 300);
        insert_media(&db.repo, id4, "2024-04-01T00:00:00Z", 400);

        db.repo
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE media SET media_type = 'video' WHERE id = ?1",
                    params![id2.as_bytes()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        db.repo.set_favorite_impl(id1, true).unwrap();
        db.repo.set_favorite_impl(id2, true).unwrap();
        db.repo.set_favorite_impl(id4, true).unwrap();
        db.repo
            .update_media_tags_impl(id1, vec!["Nature".to_string()])
            .unwrap();
        db.repo
            .update_media_tags_impl(id2, vec!["Nature".to_string()])
            .unwrap();
        db.repo
            .update_media_tags_impl(id3, vec!["City".to_string()])
            .unwrap();
        db.repo
            .update_media_tags_impl(id4, vec!["City".to_string()])
            .unwrap();

        // Favorite images only
        let fav_images = db
            .repo
            .find_all_impl(10, 0, Some("image"), true, None, None, None, false, "date")
            .unwrap();
        assert_eq!(fav_images.len(), 2); // id1, id4
        assert!(fav_images
            .iter()
            .all(|m| m.media_type == "image" && m.is_favorite));

        // Favorite + Nature tag
        let fav_nature = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                true,
                Some(vec!["Nature".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(fav_nature.len(), 2); // id1, id2

        // Image + Nature tag + favorite
        let img_nat_fav = db
            .repo
            .find_all_impl(
                10,
                0,
                Some("image"),
                true,
                Some(vec!["Nature".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(img_nat_fav.len(), 1);
        assert_eq!(img_nat_fav[0].id, id1);
    }

    // ==================== Pagination tests ====================

    #[test]
    fn test_pagination_limit_offset() {
        let db = TestDb::new("test_pagination");

        // Insert 10 items with sequential dates
        let ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
        for (i, id) in ids.iter().enumerate() {
            insert_media(
                &db.repo,
                *id,
                &format!("2024-{:02}-01T00:00:00Z", i + 1),
                (i as i64 + 1) * 100,
            );
        }

        // Page 1: limit 3, offset 0 (DESC: Oct, Sep, Aug)
        let page1 = db
            .repo
            .find_all_impl(3, 0, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].id, ids[9]); // Oct (month 10)
        assert_eq!(page1[1].id, ids[8]); // Sep
        assert_eq!(page1[2].id, ids[7]); // Aug

        // Page 2: limit 3, offset 3 (DESC: Jul, Jun, May)
        let page2 = db
            .repo
            .find_all_impl(3, 3, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(page2.len(), 3);
        assert_eq!(page2[0].id, ids[6]); // Jul

        // Page 4: limit 3, offset 9 (only 1 item left)
        let page4 = db
            .repo
            .find_all_impl(3, 9, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(page4.len(), 1);
        assert_eq!(page4[0].id, ids[0]); // Jan

        // Beyond all: offset 10
        let empty = db
            .repo
            .find_all_impl(3, 10, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_pagination_no_overlap() {
        let db = TestDb::new("test_pagination_no_overlap");

        let ids: Vec<Uuid> = (0..7).map(|_| Uuid::new_v4()).collect();
        for (i, id) in ids.iter().enumerate() {
            insert_media(
                &db.repo,
                *id,
                &format!("2024-{:02}-01T00:00:00Z", i + 1),
                100,
            );
        }

        // Fetch all pages of size 3 and verify no overlap and full coverage
        let mut all_ids = Vec::new();
        for offset in (0..10).step_by(3) {
            let page = db
                .repo
                .find_all_impl(3, offset, None, false, None, None, None, false, "date")
                .unwrap();
            for item in &page {
                assert!(!all_ids.contains(&item.id), "Duplicate item across pages");
                all_ids.push(item.id);
            }
        }
        assert_eq!(all_ids.len(), 7);
    }

    // ==================== CRUD tests ====================

    #[test]
    fn test_save_and_find_by_id() {
        let db = TestDb::new("test_save_find");

        let id = Uuid::new_v4();
        let media = crate::domain::MediaItem {
            id,
            filename: "ab/cd/test.jpg".to_string(),
            original_filename: "photo.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "abc123".to_string(),
            uploaded_at: chrono::Utc::now(),
            original_date: chrono::DateTime::parse_from_rfc3339("2024-06-15T12:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            width: Some(1920),
            height: Some(1080),
            size_bytes: 5_000_000,
            exif_json: Some(r#"{"Make":"Canon"}"#.to_string()),
            is_favorite: false,
            faces_scanned: false,
            tags: vec![],
            faces: vec![],
        };

        db.repo.save_metadata_and_vector_impl(&media, None).unwrap();

        let found = db.repo.find_by_id_impl(id).unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.filename, "ab/cd/test.jpg");
        assert_eq!(found.original_filename, "photo.jpg");
        assert_eq!(found.media_type, "image");
        assert_eq!(found.width, Some(1920));
        assert_eq!(found.height, Some(1080));
        assert_eq!(found.size_bytes, 5_000_000);
        assert_eq!(found.exif_json, Some(r#"{"Make":"Canon"}"#.to_string()));
        assert!(!found.is_favorite);
    }

    #[test]
    fn test_find_by_id_not_found() {
        let db = TestDb::new("test_find_not_found");
        let result = db.repo.find_by_id_impl(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_exists_by_phash() {
        let db = TestDb::new("test_exists_phash");

        let id = Uuid::new_v4();
        insert_media(&db.repo, id, "2024-01-01T00:00:00Z", 100);

        assert!(db.repo.exists_by_phash_impl("ph").unwrap());
        assert!(!db.repo.exists_by_phash_impl("nonexistent").unwrap());
    }

    #[test]
    fn test_delete_single() {
        let db = TestDb::new("test_delete_single");

        let id = Uuid::new_v4();
        insert_media(&db.repo, id, "2024-01-01T00:00:00Z", 100);

        assert!(db.repo.find_by_id_impl(id).unwrap().is_some());
        db.repo.delete_impl(id).unwrap();
        assert!(db.repo.find_by_id_impl(id).unwrap().is_none());
    }

    #[test]
    fn test_delete_not_found() {
        let db = TestDb::new("test_delete_not_found");
        let result = db.repo.delete_impl(Uuid::new_v4());
        assert!(matches!(result, Err(crate::domain::DomainError::NotFound)));
    }

    #[test]
    fn test_delete_many() {
        let db = TestDb::new("test_delete_many");

        let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        for id in &ids {
            insert_media(&db.repo, *id, "2024-01-01T00:00:00Z", 100);
        }

        // Delete first 3
        let deleted = db.repo.delete_many_impl(&ids[0..3]).unwrap();
        assert_eq!(deleted, 3);

        // Verify remaining
        let all = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    // ==================== Counts tests ====================

    #[test]
    fn test_media_counts() {
        let db = TestDb::new("test_media_counts");

        // Empty DB
        let counts = db.repo.media_counts_impl().unwrap();
        assert_eq!(counts.total, 0);
        assert_eq!(counts.images, 0);
        assert_eq!(counts.videos, 0);
        assert_eq!(counts.total_size_bytes, 0);

        // Add items
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 1000);
        insert_media(&db.repo, id2, "2024-02-01T00:00:00Z", 2000);
        insert_media(&db.repo, id3, "2024-03-01T00:00:00Z", 3000);

        db.repo
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE media SET media_type = 'video' WHERE id = ?1",
                    params![id3.as_bytes()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();

        let counts = db.repo.media_counts_impl().unwrap();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.images, 2);
        assert_eq!(counts.videos, 1);
        assert_eq!(counts.total_size_bytes, 6000);
    }

    // ==================== Favorites tests ====================

    // ==================== SQL injection safety tests ====================

    #[test]
    fn test_injection_via_sort_by() {
        let db = TestDb::new("test_inject_sort_by");

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 200);

        // All of these should safely fall through to the default (date) ordering,
        // not cause errors or alter behavior
        let payloads = [
            "size; DROP TABLE media; --",
            "1 OR 1=1",
            "original_date; DELETE FROM media",
            "' OR '1'='1",
            "m.id; DROP TABLE media--",
            "CASE WHEN 1=1 THEN size_bytes ELSE original_date END",
        ];

        for payload in &payloads {
            let results = db
                .repo
                .find_all_impl(10, 0, None, false, None, None, None, false, payload)
                .unwrap();
            assert_eq!(
                results.len(),
                2,
                "Injection payload should not crash: {}",
                payload
            );
            // Should be date DESC order (default fallthrough)
            assert_eq!(
                results[0].id, id2,
                "Should default to date order for: {}",
                payload
            );
        }
    }

    #[test]
    fn test_injection_via_media_type_filter() {
        let db = TestDb::new("test_inject_media_type");

        let id1 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);

        // media_type is passed as a parameterized query (?), so injection attempts
        // just become literal string comparisons that match nothing
        let payloads = [
            "image' OR '1'='1",
            "image; DROP TABLE media;--",
            "' UNION SELECT id,filename,original_filename,media_type,uploaded_at,original_date,1,size_bytes FROM media--",
        ];

        for payload in &payloads {
            let results = db
                .repo
                .find_all_impl(10, 0, Some(payload), false, None, None, None, false, "date")
                .unwrap();
            assert_eq!(
                results.len(),
                0,
                "Injection via media_type should match nothing: {}",
                payload
            );
        }

        // Normal filter still works
        let results = db
            .repo
            .find_all_impl(10, 0, Some("image"), false, None, None, None, false, "date")
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_injection_via_tag_filter() {
        let db = TestDb::new("test_inject_tags");

        let id1 = Uuid::new_v4();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        db.repo
            .update_media_tags_impl(id1, vec!["Safe".to_string()])
            .unwrap();

        // Tags are passed as parameterized IN (?), so these are literal string comparisons
        let payloads = vec![
            "Safe' OR '1'='1".to_string(),
            "') OR 1=1--".to_string(),
            "Safe'; DROP TABLE tags;--".to_string(),
        ];

        for payload in &payloads {
            let results = db
                .repo
                .find_all_impl(
                    10,
                    0,
                    None,
                    false,
                    Some(vec![payload.clone()]),
                    None,
                    None,
                    false,
                    "date",
                )
                .unwrap();
            assert_eq!(
                results.len(),
                0,
                "Injection via tags should match nothing: {}",
                payload
            );
        }

        // Normal tag filter still works
        let results = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                Some(vec!["Safe".to_string()]),
                None,
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_injection_via_sort_by_in_folder() {
        let db = TestDb::new("test_inject_sort_by_folder");

        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        db.repo.create_folder_impl(folder_id, "Test").unwrap();
        insert_media(&db.repo, id1, "2024-01-01T00:00:00Z", 100);
        insert_media(&db.repo, id2, "2024-06-15T00:00:00Z", 200);
        db.repo
            .add_media_to_folder_impl(folder_id, &[id1, id2])
            .unwrap();

        let payloads = ["size; DROP TABLE media; --", "1 OR 1=1", "' OR '1'='1"];

        for payload in &payloads {
            let results = db
                .repo
                .find_all_in_folder_impl(
                    folder_id, 10, 0, None, false, None, None, None, false, payload,
                )
                .unwrap();
            assert_eq!(
                results.len(),
                2,
                "Injection should not crash in folder: {}",
                payload
            );
            assert_eq!(
                results[0].id, id2,
                "Should default to date order for: {}",
                payload
            );
        }
    }

    // ==================== Favorites tests ====================

    #[test]
    fn test_favorite_toggle() {
        let db = TestDb::new("test_favorite_toggle");

        let id = Uuid::new_v4();
        insert_media(&db.repo, id, "2024-01-01T00:00:00Z", 100);

        // Not favorited initially
        let item = db.repo.find_by_id_impl(id).unwrap().unwrap();
        assert!(!item.is_favorite);

        // Favorite it
        db.repo.set_favorite_impl(id, true).unwrap();
        let item = db.repo.find_by_id_impl(id).unwrap().unwrap();
        assert!(item.is_favorite);

        // Double-favorite is idempotent (INSERT OR IGNORE)
        db.repo.set_favorite_impl(id, true).unwrap();
        let item = db.repo.find_by_id_impl(id).unwrap().unwrap();
        assert!(item.is_favorite);

        // Unfavorite
        db.repo.set_favorite_impl(id, false).unwrap();
        let item = db.repo.find_by_id_impl(id).unwrap().unwrap();
        assert!(!item.is_favorite);
    }

    #[test]
    fn test_filter_by_person() {
        let db = TestDb::new("test_filter_person");
        let id = Uuid::new_v4();
        insert_media(&db.repo, id, "2024-01-01T00:00:00Z", 100);
        let person_id = Uuid::new_v4();

        db.repo.with_conn(|conn| {
            conn.execute("INSERT INTO people (id, name, is_hidden, created_at) VALUES (?1, 'P', 0, '2024-01-01')", params![person_id.as_bytes()]).unwrap();
            conn.execute("INSERT INTO faces (id, media_id, box_x1, box_y1, box_x2, box_y2, person_id) VALUES (?1, ?2, 0, 0, 1, 1, ?3)",
                params![Uuid::new_v4().as_bytes(), id.as_bytes(), person_id.as_bytes()]).unwrap();
            Ok(())
        }).unwrap();

        let results = db
            .repo
            .find_all_impl(
                10,
                0,
                None,
                false,
                None,
                Some(person_id),
                None,
                false,
                "date",
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }

    #[test]
    fn test_filter_by_cluster() {
        let db = TestDb::new("test_filter_cluster");
        let id = Uuid::new_v4();
        insert_media(&db.repo, id, "2024-01-01T00:00:00Z", 100);

        db.repo.with_conn(|conn| {
            conn.execute("INSERT INTO faces (id, media_id, box_x1, box_y1, box_x2, box_y2, cluster_id) VALUES (?1, ?2, 0, 0, 1, 1, ?3)",
                params![Uuid::new_v4().as_bytes(), id.as_bytes(), 999]).unwrap();
            Ok(())
        }).unwrap();

        let results = db
            .repo
            .find_all_impl(10, 0, None, false, None, None, Some(999), false, "date")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }
}
