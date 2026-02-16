use crate::domain::{DomainError, Folder, MediaSummary};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use super::{load_tags_bulk, SqliteRepository};

impl SqliteRepository {
    pub(crate) fn create_folder_impl(&self, id: Uuid, name: &str) -> Result<Folder, DomainError> {
        let now = Utc::now();
        self.with_conn(|conn| {
            let max_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), -1) FROM folders",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let sort_order = max_order + 1;

            conn.execute(
                "INSERT INTO folders (id, name, created_at, sort_order) VALUES (?1, ?2, ?3, ?4)",
                params![id.as_bytes(), name, now.to_rfc3339(), sort_order],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            Ok(Folder {
                id,
                name: name.to_string(),
                created_at: now,
                item_count: 0,
                sort_order,
            })
        })
    }

    pub(crate) fn get_folder_impl(&self, id: Uuid) -> Result<Option<Folder>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT f.id, f.name, f.created_at, COALESCE(c.cnt, 0), f.sort_order
                 FROM folders f
                 LEFT JOIN (SELECT folder_id, COUNT(*) as cnt FROM folder_media WHERE folder_id = ?1) c
                   ON c.folder_id = f.id
                 WHERE f.id = ?1",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let result = stmt.query_row(params![id.as_bytes()], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let name: String = row.get(1)?;
                let created_at_str: String = row.get(2)?;
                let item_count: i64 = row.get(3)?;
                let sort_order: i64 = row.get(4)?;

                let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Blob,
                        Box::new(e),
                    )
                })?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?
                    .with_timezone(&Utc);

                Ok(Folder {
                    id,
                    name,
                    created_at,
                    item_count,
                    sort_order,
                })
            });

            match result {
                Ok(folder) => Ok(Some(folder)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    pub(crate) fn list_folders_impl(&self) -> Result<Vec<Folder>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT f.id, f.name, f.created_at, COALESCE(c.cnt, 0), f.sort_order
                 FROM folders f
                 LEFT JOIN (SELECT folder_id, COUNT(*) as cnt FROM folder_media GROUP BY folder_id) c
                   ON c.folder_id = f.id
                 ORDER BY f.sort_order ASC, f.name COLLATE NOCASE",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let name: String = row.get(1)?;
                    let created_at_str: String = row.get(2)?;
                    let item_count: i64 = row.get(3)?;
                    let sort_order: i64 = row.get(4)?;

                    let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            Box::new(e),
                        )
                    })?;
                    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                2,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .with_timezone(&Utc);

                    Ok(Folder {
                        id,
                        name,
                        created_at,
                        item_count,
                        sort_order,
                    })
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut folders = Vec::new();
            for row in rows {
                folders.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(folders)
        })
    }

    pub(crate) fn delete_folder_impl(&self, id: Uuid) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM folder_media WHERE folder_id = ?1",
                params![id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;
            let deleted = conn
                .execute("DELETE FROM folders WHERE id = ?1", params![id.as_bytes()])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            if deleted == 0 {
                return Err(DomainError::NotFound);
            }
            Ok(())
        })
    }

    pub(crate) fn rename_folder_impl(&self, id: Uuid, name: &str) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let updated = conn
                .execute(
                    "UPDATE folders SET name = ?1 WHERE id = ?2",
                    params![name, id.as_bytes()],
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;
            if updated == 0 {
                return Err(DomainError::NotFound);
            }
            Ok(())
        })
    }

    pub(crate) fn reorder_folders_impl(&self, order: &[(Uuid, i64)]) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("UPDATE folders SET sort_order = ?1 WHERE id = ?2")
                .map_err(|e| DomainError::Database(e.to_string()))?;
            for (id, sort_order) in order {
                stmt.execute(params![sort_order, id.as_bytes()])
                    .map_err(|e| DomainError::Database(e.to_string()))?;
            }
            Ok(())
        })
    }

    pub(crate) fn add_media_to_folder_impl(
        &self,
        folder_id: Uuid,
        media_ids: &[Uuid],
    ) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            let now = Utc::now().to_rfc3339();
            let mut added = 0usize;
            for media_id in media_ids {
                let res = conn.execute(
                    "INSERT OR IGNORE INTO folder_media (folder_id, media_id, added_at) VALUES (?1, ?2, ?3)",
                    params![folder_id.as_bytes(), media_id.as_bytes(), now],
                );
                match res {
                    Ok(n) => added += n,
                    Err(e) => return Err(DomainError::Database(e.to_string())),
                }
            }
            Ok(added)
        })
    }

    pub(crate) fn remove_media_from_folder_impl(
        &self,
        folder_id: Uuid,
        media_ids: &[Uuid],
    ) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            let mut removed = 0usize;
            for media_id in media_ids {
                let n = conn
                    .execute(
                        "DELETE FROM folder_media WHERE folder_id = ?1 AND media_id = ?2",
                        params![folder_id.as_bytes(), media_id.as_bytes()],
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                removed += n;
            }
            Ok(removed)
        })
    }

    pub(crate) fn find_all_in_folder_impl(
        &self,
        folder_id: Uuid,
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
                           JOIN folder_media fm ON fm.media_id = m.id
                           LEFT JOIN favorites f ON f.media_id = m.id
                           WHERE fm.folder_id = ?"
                .to_string();

            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(folder_id.as_bytes().to_vec())];

            if let Some(mt) = media_type {
                sql.push_str(" AND m.media_type = ?");
                params_vec.push(Box::new(mt.to_string()));
            }

            if favorite {
                sql.push_str(" AND f.media_id IS NOT NULL");
            }

            // Tag filtering: media must have ALL specified tags
            if let Some(ref tag_list) = tags {
                if !tag_list.is_empty() {
                    for tag in tag_list {
                        sql.push_str(" AND EXISTS (SELECT 1 FROM media_tags mt2 JOIN tags t2 ON t2.id = mt2.tag_id WHERE mt2.media_id = m.id AND t2.name = ?)");
                        params_vec.push(Box::new(tag.clone()));
                    }
                }
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

    pub(crate) fn get_folder_media_files_impl(
        &self,
        folder_id: Uuid,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, m.size_bytes
                 FROM media m
                 JOIN folder_media fm ON fm.media_id = m.id
                 WHERE fm.folder_id = ?1",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![folder_id.as_bytes()], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let filename: String = row.get(1)?;
                    let original_filename: String = row.get(2)?;
                    let media_type: String = row.get(3)?;
                    let timestamp_str: String = row.get(4)?;
                    let original_date_str: String = row.get(5)?;
                    let size_bytes: i64 = row.get(6)?;

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

                    Ok(MediaSummary {
                        id,
                        filename,
                        original_filename,
                        media_type,
                        uploaded_at,
                        original_date,
                        size_bytes,
                        is_favorite: false,
                        tags: vec![],
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
}
