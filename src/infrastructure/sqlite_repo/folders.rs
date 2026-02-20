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
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;

            tx.execute(
                "DELETE FROM folder_media WHERE folder_id = ?1",
                params![id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            let deleted = tx
                .execute("DELETE FROM folders WHERE id = ?1", params![id.as_bytes()])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            if deleted == 0 {
                return Err(DomainError::NotFound);
            }

            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
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
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;

            {
                let mut stmt = tx
                    .prepare("UPDATE folders SET sort_order = ?1 WHERE id = ?2")
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                for (id, sort_order) in order {
                    stmt.execute(params![sort_order, id.as_bytes()])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                }
            }

            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn add_media_to_folder_impl(
        &self,
        folder_id: Uuid,
        media_ids: &[Uuid],
    ) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| DomainError::Database(e.to_string()))?;
            let now = Utc::now().to_rfc3339();
            let mut added = 0usize;
            
            {
                let mut stmt = tx.prepare("INSERT OR IGNORE INTO folder_media (folder_id, media_id, added_at) VALUES (?1, ?2, ?3)")
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                
                for media_id in media_ids {
                    let res = stmt.execute(params![folder_id.as_bytes(), media_id.as_bytes(), now]);
                    match res {
                        Ok(n) => added += n,
                        Err(e) => return Err(DomainError::Database(e.to_string())),
                    }
                }
            }

            tx.commit().map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(added)
        })
    }

    pub(crate) fn remove_media_from_folder_impl(
        &self,
        folder_id: Uuid,
        media_ids: &[Uuid],
    ) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let mut removed = 0usize;

            {
                let mut stmt = tx
                    .prepare("DELETE FROM folder_media WHERE folder_id = ?1 AND media_id = ?2")
                    .map_err(|e| DomainError::Database(e.to_string()))?;

                for media_id in media_ids {
                    let n = stmt
                        .execute(params![folder_id.as_bytes(), media_id.as_bytes()])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    removed += n;
                }
            }

            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
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
        person_id: Option<Uuid>,
        cluster_id: Option<i64>,
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

            // Tag filtering: media must have ANY of the specified tags (OR)
            
            if let Some(pid) = person_id {
                sql.push_str(" AND EXISTS (SELECT 1 FROM faces f2 WHERE f2.media_id = m.id AND f2.person_id = ?)");
                params_vec.push(Box::new(pid.as_bytes().to_vec()));
            }

            if let Some(cid) = cluster_id {
                sql.push_str(" AND EXISTS (SELECT 1 FROM faces f2 WHERE f2.media_id = m.id AND f2.cluster_id = ?)");
                params_vec.push(Box::new(cid));
            }

            if let Some(ref tag_list) = tags {
                if !tag_list.is_empty() {
                    let placeholders: Vec<String> = tag_list.iter().map(|_| "?".to_string()).collect();
                    sql.push_str(&format!(
                        " AND EXISTS (SELECT 1 FROM media_tags mt2 JOIN tags t2 ON t2.id = mt2.tag_id WHERE mt2.media_id = m.id AND t2.name IN ({}))",
                        placeholders.join(", ")
                    ));
                    for tag in tag_list {
                        params_vec.push(Box::new(tag.clone()));
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::super::TestDb;
    use rusqlite::params;
    use uuid::Uuid;

    /// Insert a media row with a given original_date, size, and media_type.
    fn insert_media(db: &TestDb, id: Uuid, date: &str, size: i64, media_type: &str) {
        db.repo.with_conn(|conn| {
            conn.execute(
                "INSERT INTO media (id, filename, original_filename, media_type, size_bytes, phash, uploaded_at, original_date)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'ph', '2024-01-01T00:00:00Z', ?6)",
                params![
                    id.as_bytes(),
                    format!("{}.jpg", id),
                    format!("{}.jpg", id),
                    media_type,
                    size,
                    date,
                ],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    }

    // ==================== Folder CRUD ====================

    #[test]
    fn test_create_and_get_folder() {
        let db = TestDb::new("test_create_folder");
        let id = Uuid::new_v4();

        let folder = db.repo.create_folder_impl(id, "My Folder").unwrap();
        assert_eq!(folder.id, id);
        assert_eq!(folder.name, "My Folder");
        assert_eq!(folder.item_count, 0);

        let found = db.repo.get_folder_impl(id).unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.name, "My Folder");
    }

    #[test]
    fn test_get_folder_not_found() {
        let db = TestDb::new("test_get_folder_not_found");
        let result = db.repo.get_folder_impl(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_folders_ordered() {
        let db = TestDb::new("test_list_folders");
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        db.repo.create_folder_impl(id1, "Zebras").unwrap();
        db.repo.create_folder_impl(id2, "Apples").unwrap();
        db.repo.create_folder_impl(id3, "Mango").unwrap();

        let folders = db.repo.list_folders_impl().unwrap();
        assert_eq!(folders.len(), 3);
        // Ordered by sort_order (creation order), not alphabetically
        assert_eq!(folders[0].id, id1);
        assert_eq!(folders[1].id, id2);
        assert_eq!(folders[2].id, id3);
    }

    #[test]
    fn test_rename_folder() {
        let db = TestDb::new("test_rename_folder");
        let id = Uuid::new_v4();
        db.repo.create_folder_impl(id, "Old Name").unwrap();

        db.repo.rename_folder_impl(id, "New Name").unwrap();
        let found = db.repo.get_folder_impl(id).unwrap().unwrap();
        assert_eq!(found.name, "New Name");
    }

    #[test]
    fn test_rename_nonexistent_folder() {
        let db = TestDb::new("test_rename_nonexistent");
        let result = db.repo.rename_folder_impl(Uuid::new_v4(), "Name");
        assert!(matches!(result, Err(crate::domain::DomainError::NotFound)));
    }

    #[test]
    fn test_delete_folder() {
        let db = TestDb::new("test_delete_folder");
        let id = Uuid::new_v4();
        db.repo.create_folder_impl(id, "To Delete").unwrap();

        db.repo.delete_folder_impl(id).unwrap();
        assert!(db.repo.get_folder_impl(id).unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_folder() {
        let db = TestDb::new("test_delete_nonexistent_folder");
        let result = db.repo.delete_folder_impl(Uuid::new_v4());
        assert!(matches!(result, Err(crate::domain::DomainError::NotFound)));
    }

    #[test]
    fn test_reorder_folders() {
        let db = TestDb::new("test_reorder_folders");
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        db.repo.create_folder_impl(id1, "First").unwrap();
        db.repo.create_folder_impl(id2, "Second").unwrap();
        db.repo.create_folder_impl(id3, "Third").unwrap();

        // Reverse the order
        db.repo
            .reorder_folders_impl(&[(id3, 0), (id2, 1), (id1, 2)])
            .unwrap();

        let folders = db.repo.list_folders_impl().unwrap();
        assert_eq!(folders[0].id, id3);
        assert_eq!(folders[1].id, id2);
        assert_eq!(folders[2].id, id1);
    }

    // ==================== Folder media membership ====================

    #[test]
    fn test_add_and_remove_media_from_folder() {
        let db = TestDb::new("test_add_remove_folder_media");
        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Test").unwrap();
        insert_media(&db, id1, "2024-01-01T00:00:00Z", 100, "image");
        insert_media(&db, id2, "2024-02-01T00:00:00Z", 200, "image");
        insert_media(&db, id3, "2024-03-01T00:00:00Z", 300, "image");

        // Add 3 items
        let added = db
            .repo
            .add_media_to_folder_impl(folder_id, &[id1, id2, id3])
            .unwrap();
        assert_eq!(added, 3);

        // Verify folder item count
        let folder = db.repo.get_folder_impl(folder_id).unwrap().unwrap();
        assert_eq!(folder.item_count, 3);

        // Verify listing
        let items = db
            .repo
            .find_all_in_folder_impl(folder_id, 10, 0, None, false, None, false, "date")
            .unwrap();
        assert_eq!(items.len(), 3);

        // Remove 1
        let removed = db
            .repo
            .remove_media_from_folder_impl(folder_id, &[id2])
            .unwrap();
        assert_eq!(removed, 1);

        let items = db
            .repo
            .find_all_in_folder_impl(folder_id, 10, 0, None, false, None, false, "date")
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(!items.iter().any(|m| m.id == id2));
    }

    #[test]
    fn test_add_media_to_folder_idempotent() {
        let db = TestDb::new("test_add_folder_idempotent");
        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Test").unwrap();
        insert_media(&db, id1, "2024-01-01T00:00:00Z", 100, "image");

        // Add once
        let added = db.repo.add_media_to_folder_impl(folder_id, &[id1]).unwrap();
        assert_eq!(added, 1);

        // Add again — INSERT OR IGNORE, so 0 new
        let added = db.repo.add_media_to_folder_impl(folder_id, &[id1]).unwrap();
        assert_eq!(added, 0);

        // Still only 1 item
        let items = db
            .repo
            .find_all_in_folder_impl(folder_id, 10, 0, None, false, None, false, "date")
            .unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_delete_folder_cleans_up_membership() {
        let db = TestDb::new("test_delete_folder_cleanup");
        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Test").unwrap();
        insert_media(&db, id1, "2024-01-01T00:00:00Z", 100, "image");
        db.repo.add_media_to_folder_impl(folder_id, &[id1]).unwrap();

        // Delete folder
        db.repo.delete_folder_impl(folder_id).unwrap();

        // Media should still exist (folder deletion doesn't delete media)
        let found = db.repo.find_by_id_impl(id1).unwrap();
        assert!(found.is_some());

        // folder_media rows should be gone
        db.repo
            .with_conn(|conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM folder_media WHERE folder_id = ?1",
                        params![folder_id.as_bytes()],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(count, 0);
                Ok(())
            })
            .unwrap();
    }

    // ==================== Folder filtering ====================

    #[test]
    fn test_folder_filter_by_media_type() {
        let db = TestDb::new("test_folder_filter_type");
        let folder_id = Uuid::new_v4();
        let img = Uuid::new_v4();
        let vid = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Mixed").unwrap();
        insert_media(&db, img, "2024-01-01T00:00:00Z", 100, "image");
        insert_media(&db, vid, "2024-02-01T00:00:00Z", 200, "video");
        db.repo
            .add_media_to_folder_impl(folder_id, &[img, vid])
            .unwrap();

        let images = db
            .repo
            .find_all_in_folder_impl(folder_id, 10, 0, Some("image"), false, None, false, "date")
            .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].id, img);

        let videos = db
            .repo
            .find_all_in_folder_impl(folder_id, 10, 0, Some("video"), false, None, false, "date")
            .unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, vid);
    }

    #[test]
    fn test_folder_filter_by_favorite() {
        let db = TestDb::new("test_folder_filter_fav");
        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Favs").unwrap();
        insert_media(&db, id1, "2024-01-01T00:00:00Z", 100, "image");
        insert_media(&db, id2, "2024-02-01T00:00:00Z", 200, "image");
        db.repo
            .add_media_to_folder_impl(folder_id, &[id1, id2])
            .unwrap();
        db.repo.set_favorite_impl(id1, true).unwrap();

        let favs = db
            .repo
            .find_all_in_folder_impl(folder_id, 10, 0, None, true, None, false, "date")
            .unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, id1);
    }

    #[test]
    fn test_folder_filter_by_tags() {
        let db = TestDb::new("test_folder_filter_tags");
        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Tagged").unwrap();
        insert_media(&db, id1, "2024-01-01T00:00:00Z", 100, "image");
        insert_media(&db, id2, "2024-02-01T00:00:00Z", 200, "image");
        db.repo
            .add_media_to_folder_impl(folder_id, &[id1, id2])
            .unwrap();
        db.repo
            .update_media_tags_impl(id1, vec!["Landscape".to_string()])
            .unwrap();

        let tagged = db
            .repo
            .find_all_in_folder_impl(
                folder_id,
                10,
                0,
                None,
                false,
                Some(vec!["Landscape".to_string()]),
                false,
                "date",
            )
            .unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, id1);
    }

    #[test]
    fn test_folder_pagination() {
        let db = TestDb::new("test_folder_pagination");
        let folder_id = Uuid::new_v4();
        db.repo.create_folder_impl(folder_id, "Big").unwrap();

        let ids: Vec<Uuid> = (0..7).map(|_| Uuid::new_v4()).collect();
        for (i, id) in ids.iter().enumerate() {
            insert_media(
                &db,
                *id,
                &format!("2024-{:02}-01T00:00:00Z", i + 1),
                100,
                "image",
            );
        }
        db.repo.add_media_to_folder_impl(folder_id, &ids).unwrap();

        // Page 1
        let p1 = db
            .repo
            .find_all_in_folder_impl(folder_id, 3, 0, None, false, None, false, "date")
            .unwrap();
        assert_eq!(p1.len(), 3);

        // Page 2
        let p2 = db
            .repo
            .find_all_in_folder_impl(folder_id, 3, 3, None, false, None, false, "date")
            .unwrap();
        assert_eq!(p2.len(), 3);

        // No overlap
        let p1_ids: Vec<Uuid> = p1.iter().map(|m| m.id).collect();
        for item in &p2 {
            assert!(!p1_ids.contains(&item.id));
        }

        // Page 3 — only 1 left
        let p3 = db
            .repo
            .find_all_in_folder_impl(folder_id, 3, 6, None, false, None, false, "date")
            .unwrap();
        assert_eq!(p3.len(), 1);
    }

    #[test]
    fn test_get_folder_media_files() {
        let db = TestDb::new("test_folder_media_files");
        let folder_id = Uuid::new_v4();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        db.repo.create_folder_impl(folder_id, "Download").unwrap();
        insert_media(&db, id1, "2024-01-01T00:00:00Z", 1000, "image");
        insert_media(&db, id2, "2024-02-01T00:00:00Z", 2000, "video");
        db.repo
            .add_media_to_folder_impl(folder_id, &[id1, id2])
            .unwrap();

        let files = db.repo.get_folder_media_files_impl(folder_id).unwrap();
        assert_eq!(files.len(), 2);
        let total_size: i64 = files.iter().map(|f| f.size_bytes).sum();
        assert_eq!(total_size, 3000);
    }
}
