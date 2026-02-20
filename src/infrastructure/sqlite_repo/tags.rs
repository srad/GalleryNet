use crate::domain::DomainError;
use rusqlite::params;
use uuid::Uuid;

use super::SqliteRepository;

impl SqliteRepository {
    pub(crate) fn get_all_tags_impl(
        &self,
    ) -> Result<Vec<crate::domain::models::TagCount>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT t.name, COUNT(mt.media_id)
                     FROM tags t
                     JOIN media_tags mt ON mt.tag_id = t.id
                     GROUP BY t.id, t.name
                     ORDER BY t.name",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
                    Ok(crate::domain::models::TagCount {
                        name: row.get(0)?,
                        count: row.get(1)?,
                    })
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut tags = Vec::new();
            for row in rows {
                tags.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(tags)
        })
    }

    pub(crate) fn get_tag_model_impl(
        &self,
        tag_id: i64,
    ) -> Result<Option<crate::domain::TrainedTagModel>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT weights, bias, platt_a, platt_b FROM tag_models WHERE tag_id = ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let result = stmt.query_row(params![tag_id], |row| {
                let weights_bytes: Vec<u8> = row.get(0)?;
                let bias: f64 = row.get(1)?;
                let platt_a: f64 = row.get(2)?;
                let platt_b: f64 = row.get(3)?;

                if weights_bytes.len() % 8 != 0 {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Invalid weights length",
                        )),
                    ));
                }

                let mut weights = Vec::with_capacity(weights_bytes.len() / 8);
                for chunk in weights_bytes.chunks_exact(8) {
                    let arr: [u8; 8] = chunk.try_into().unwrap();
                    weights.push(f64::from_ne_bytes(arr));
                }

                Ok(crate::domain::TrainedTagModel {
                    weights,
                    bias,
                    platt_a,
                    platt_b,
                })
            });

            match result {
                Ok(model) => Ok(Some(model)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    pub(crate) fn save_tag_model_impl(
        &self,
        tag_id: i64,
        weights: &[f64],
        bias: f64,
        platt_a: f64,
        platt_b: f64,
        trained_at_count: usize,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let weights_bytes: Vec<u8> = weights
                .iter()
                .flat_map(|&w| w.to_ne_bytes().to_vec())
                .collect();

            conn.execute(
                "INSERT OR REPLACE INTO tag_models (tag_id, weights, bias, platt_a, platt_b, trained_at_count, version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE((SELECT version FROM tag_models WHERE tag_id = ?1), 0) + 1)",
                params![tag_id, weights_bytes, bias, platt_a, platt_b, trained_at_count as i64],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_last_trained_count_impl(&self, tag_id: i64) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT trained_at_count FROM tag_models WHERE tag_id = ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let count: i64 = stmt
                .query_row(params![tag_id], |row| row.get(0))
                .unwrap_or(0);
            Ok(count as usize)
        })
    }

    pub(crate) fn get_tags_with_manual_counts_impl(
        &self,
    ) -> Result<Vec<(i64, String, usize)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT t.id, t.name, COUNT(mt.media_id) 
                     FROM tags t
                     JOIN media_tags mt ON mt.tag_id = t.id
                     WHERE mt.is_auto = 0
                     GROUP BY t.id, t.name",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as usize))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn get_tags_with_auto_counts_impl(
        &self,
    ) -> Result<Vec<(i64, String, usize)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT t.id, t.name, COUNT(mt.media_id) 
                     FROM tags t
                     JOIN media_tags mt ON mt.tag_id = t.id
                     WHERE mt.is_auto = 1
                     GROUP BY t.id, t.name",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as usize))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn count_auto_tags_impl(
        &self,
        folder_id: Option<Uuid>,
    ) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            let sql = match folder_id {
                Some(_) => {
                    "SELECT COUNT(*) FROM media_tags mt 
                            JOIN folder_media fm ON fm.media_id = mt.media_id 
                            WHERE mt.is_auto = 1 AND fm.folder_id = ?1"
                }
                None => "SELECT COUNT(*) FROM media_tags WHERE is_auto = 1",
            };

            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let count: i64 = match folder_id {
                Some(id) => stmt
                    .query_row(params![id.as_bytes()], |row| row.get(0))
                    .map_err(|e| DomainError::Database(e.to_string()))?,
                None => stmt
                    .query_row([], |row| row.get(0))
                    .map_err(|e| DomainError::Database(e.to_string()))?,
            };

            Ok(count as usize)
        })
    }

    pub(crate) fn get_manual_positives_impl(&self, tag_id: i64) -> Result<Vec<Uuid>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT media_id FROM media_tags WHERE tag_id = ?1 AND is_auto = 0")
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![tag_id], |row| {
                    let bytes: Vec<u8> = row.get(0)?;
                    Uuid::from_slice(&bytes).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            Box::new(e),
                        )
                    })
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn update_auto_tags_impl(
        &self,
        tag_id: i64,
        media_ids_with_scores: &[(Uuid, f64)],
        scope_media_ids: Option<&[Uuid]>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Delete existing auto-tags for this tag_id within the scope
            match scope_media_ids {
                Some(ids) => {
                    let mut del_stmt = tx
                        .prepare("DELETE FROM media_tags WHERE tag_id = ?1 AND media_id = ?2 AND is_auto = 1")
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    for id in ids {
                        del_stmt.execute(params![tag_id, id.as_bytes()])
                            .map_err(|e| DomainError::Database(e.to_string()))?;
                    }
                }
                None => {
                    tx.execute(
                        "DELETE FROM media_tags WHERE tag_id = ?1 AND is_auto = 1",
                        params![tag_id],
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                }
            }

            // Insert new ones
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT INTO media_tags (media_id, tag_id, is_auto, confidence) 
                         VALUES (?1, ?2, 1, ?3)
                         ON CONFLICT(media_id, tag_id) DO UPDATE SET 
                            confidence = excluded.confidence 
                         WHERE is_auto = 1",
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;

                for (id, score) in media_ids_with_scores {
                    stmt.execute(params![id.as_bytes(), tag_id, score])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                }
            }

            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_tag_name_by_id_impl(
        &self,
        tag_id: i64,
    ) -> Result<Option<String>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT name FROM tags WHERE id = ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let mut rows = stmt
                .query_map(params![tag_id], |row| row.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;

            if let Some(row) = rows.next() {
                Ok(Some(row.map_err(|e| DomainError::Database(e.to_string()))?))
            } else {
                Ok(None)
            }
        })
    }

    pub(crate) fn get_tag_id_by_name_impl(&self, name: &str) -> Result<Option<i64>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id FROM tags WHERE name = ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let mut rows = stmt
                .query_map(params![name], |row| row.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;

            if let Some(row) = rows.next() {
                Ok(Some(row.map_err(|e| DomainError::Database(e.to_string()))?))
            } else {
                Ok(None)
            }
        })
    }

    pub(crate) fn get_all_ids_with_tag_impl(&self, tag_id: i64) -> Result<Vec<Uuid>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT media_id FROM media_tags WHERE tag_id = ?1")?;

            let rows = stmt.query_map(params![tag_id], |row| {
                let bytes: Vec<u8> = row.get(0)?;
                Uuid::from_slice(&bytes).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Blob,
                        Box::new(e),
                    )
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
    }

    pub(crate) fn update_media_tags_impl(
        &self,
        id: Uuid,
        tags: Vec<String>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Remove existing tags for this media
            tx.execute(
                "DELETE FROM media_tags WHERE media_id = ?1",
                params![id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            // Insert new tags
            {
                let mut insert_tag = tx
                    .prepare("INSERT OR IGNORE INTO tags (name) VALUES (?1)")
                    .unwrap();
                let mut select_tag = tx.prepare("SELECT id FROM tags WHERE name = ?1").unwrap();
                let mut insert_mt = tx
                    .prepare("INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?1, ?2)")
                    .unwrap();

                for tag_name in &tags {
                    let trimmed = tag_name.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    insert_tag
                        .execute(params![trimmed])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    let tag_id: i64 = select_tag
                        .query_row(params![trimmed], |row| row.get(0))
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    insert_mt
                        .execute(params![id.as_bytes(), tag_id])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                }
            }

            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn update_media_tags_batch_impl(
        &self,
        ids: &[Uuid],
        tags: &[String],
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut tag_ids: Vec<i64> = Vec::with_capacity(tags.len());
            {
                let mut insert_tag = tx
                    .prepare("INSERT OR IGNORE INTO tags (name) VALUES (?1)")
                    .unwrap();
                let mut select_tag = tx.prepare("SELECT id FROM tags WHERE name = ?1").unwrap();

                for tag_name in tags {
                    let trimmed = tag_name.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    insert_tag
                        .execute(params![trimmed])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    let tag_id: i64 = select_tag
                        .query_row(params![trimmed], |row| row.get(0))
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    tag_ids.push(tag_id);
                }
            }

            {
                let mut delete_mt = tx
                    .prepare("DELETE FROM media_tags WHERE media_id = ?1")
                    .unwrap();
                let mut insert_mt = tx
                    .prepare("INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?1, ?2)")
                    .unwrap();

                for id in ids {
                    delete_mt
                        .execute(params![id.as_bytes()])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    for &tag_id in &tag_ids {
                        insert_mt
                            .execute(params![id.as_bytes(), tag_id])
                            .map_err(|e| DomainError::Database(e.to_string()))?;
                    }
                }
            }

            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::TestDb;
    use super::*;
    use crate::infrastructure::sqlite_repo::load_tags_for_media;
    use uuid::Uuid;

    /// Insert a standard test media row.
    fn insert_test_media(repo: &super::super::SqliteRepository, media_id: Uuid) {
        repo.with_conn(|conn| {
            conn.execute(
                "INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date)
                 VALUES (?1, 'test.jpg', 'test.jpg', 100, 'abc', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                params![media_id.as_bytes()],
            ).unwrap();
            Ok(())
        }).unwrap();
    }

    #[test]
    fn test_manual_tag_protection() {
        let db = TestDb::new("test_protection");
        let media_id = Uuid::new_v4();

        insert_test_media(&db.repo, media_id);
        db.repo
            .with_conn(|conn| {
                conn.execute("INSERT INTO tags (name) VALUES ('Nature')", [])
                    .unwrap();
                Ok(())
            })
            .unwrap();

        let tag_id = db.repo.get_tag_id_by_name_impl("Nature").unwrap().unwrap();

        // Add manual tag
        db.repo
            .update_media_tags_impl(media_id, vec!["Nature".to_string()])
            .unwrap();

        // Attempt to auto-tag
        db.repo
            .update_auto_tags_impl(tag_id, &[(media_id, 0.9)], None)
            .unwrap();

        // Verify: manual tag should STILL be manual
        db.repo
            .with_conn(|conn| {
                let tags = load_tags_for_media(conn, media_id.as_bytes());
                assert_eq!(tags.len(), 1);
                assert_eq!(tags[0].is_auto, false);
                assert!(tags[0].confidence.is_none());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_auto_tag_replacement() {
        let db = TestDb::new("test_replacement");
        let media_id = Uuid::new_v4();

        insert_test_media(&db.repo, media_id);
        db.repo
            .with_conn(|conn| {
                conn.execute("INSERT INTO tags (name) VALUES ('Dog')", [])
                    .unwrap();
                Ok(())
            })
            .unwrap();

        let tag_id = db.repo.get_tag_id_by_name_impl("Dog").unwrap().unwrap();

        // 1. Initial auto-tag
        db.repo
            .update_auto_tags_impl(tag_id, &[(media_id, 0.6)], None)
            .unwrap();

        db.repo
            .with_conn(|conn| {
                let tags = load_tags_for_media(conn, media_id.as_bytes());
                assert_eq!(tags[0].confidence, Some(0.6));
                Ok(())
            })
            .unwrap();

        // 2. Update auto-tag
        db.repo
            .update_auto_tags_impl(tag_id, &[(media_id, 0.85)], None)
            .unwrap();

        db.repo
            .with_conn(|conn| {
                let tags = load_tags_for_media(conn, media_id.as_bytes());
                assert_eq!(tags[0].confidence, Some(0.85));
                assert_eq!(tags[0].is_auto, true);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_multi_tag_isolation() {
        let db = TestDb::new("test_isolation");
        let media_id = Uuid::new_v4();

        insert_test_media(&db.repo, media_id);
        db.repo
            .with_conn(|conn| {
                conn.execute("INSERT INTO tags (name) VALUES ('TagA')", [])
                    .unwrap();
                conn.execute("INSERT INTO tags (name) VALUES ('TagB')", [])
                    .unwrap();
                Ok(())
            })
            .unwrap();

        let id_a = db.repo.get_tag_id_by_name_impl("TagA").unwrap().unwrap();
        let id_b = db.repo.get_tag_id_by_name_impl("TagB").unwrap().unwrap();

        // 1. Assign both as auto
        db.repo
            .update_auto_tags_impl(id_a, &[(media_id, 0.7)], None)
            .unwrap();
        db.repo
            .update_auto_tags_impl(id_b, &[(media_id, 0.8)], None)
            .unwrap();

        db.repo
            .with_conn(|conn| {
                let tags = load_tags_for_media(conn, media_id.as_bytes());
                assert_eq!(tags.len(), 2);
                Ok(())
            })
            .unwrap();

        // 2. Update ONLY TagA
        db.repo
            .update_auto_tags_impl(id_a, &[(media_id, 0.9)], None)
            .unwrap();

        // 3. Verify: TagB should STILL be there and TagA should be updated
        db.repo
            .with_conn(|conn| {
                let tags = load_tags_for_media(conn, media_id.as_bytes());
                assert_eq!(tags.len(), 2);
                let t_a = tags.iter().find(|t| t.name == "TagA").unwrap();
                let t_b = tags.iter().find(|t| t.name == "TagB").unwrap();
                assert_eq!(t_a.confidence, Some(0.9));
                assert_eq!(t_b.confidence, Some(0.8));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_negative_sampling_integrity() {
        let db = TestDb::new("test_sampling");

        let media_ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();

        db.repo.with_conn(|conn| {
            for (i, id) in media_ids.iter().enumerate() {
                conn.execute(
                    "INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date)
                     VALUES (?1, ?2, ?2, 100, 'abc', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                    params![id.as_bytes(), format!("test{}.jpg", i)],
                ).unwrap();
                // Add a dummy embedding
                conn.execute(
                    "INSERT INTO vec_media (rowid, embedding) VALUES (?1, ?2)",
                    params![conn.last_insert_rowid(), vec![0u8; 1280*4]],
                ).unwrap();
            }
            Ok(())
        }).unwrap();

        // Try to get 5 random embeddings, excluding the first 5
        let exclude = &media_ids[0..5];
        let samples = db.repo.get_random_embeddings_impl(10, exclude).unwrap();

        for (id, _) in samples {
            assert!(!exclude.contains(&id), "Sampled an excluded ID: {}", id);
        }
    }

    #[test]
    fn test_batch_tagging() {
        let db = TestDb::new("test_batch");
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

        db.repo.with_conn(|conn| {
            for (i, id) in ids.iter().enumerate() {
                conn.execute(
                    "INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date)
                     VALUES (?1, ?2, ?2, 100, 'abc', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                    params![id.as_bytes(), format!("batch{}.jpg", i)],
                ).unwrap();
            }
            Ok(())
        }).unwrap();

        // Batch apply two tags
        db.repo
            .update_media_tags_batch_impl(&ids, &["Batch1".to_string(), "Batch2".to_string()])
            .unwrap();

        db.repo
            .with_conn(|conn| {
                for id in ids {
                    let tags = load_tags_for_media(conn, id.as_bytes());
                    assert_eq!(tags.len(), 2);
                    assert!(tags.iter().any(|t| t.name == "Batch1"));
                    assert!(tags.iter().any(|t| t.name == "Batch2"));
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_tag_counts_and_positives() {
        let db = TestDb::new("test_counts");
        let media_id = Uuid::new_v4();

        insert_test_media(&db.repo, media_id);

        db.repo
            .update_media_tags_impl(media_id, vec!["CountMe".to_string()])
            .unwrap();

        let counts = db.repo.get_tags_with_manual_counts_impl().unwrap();
        let item = counts
            .iter()
            .find(|(_, name, _)| name == "CountMe")
            .unwrap();
        assert_eq!(item.2, 1); // 1 manual tag

        let positives = db.repo.get_manual_positives_impl(item.0).unwrap();
        assert_eq!(positives.len(), 1);
        assert_eq!(positives[0], media_id);
    }

    #[test]
    fn test_model_persistence() {
        let db = TestDb::new("test_model");

        db.repo
            .with_conn(|conn| {
                conn.execute("INSERT INTO tags (id, name) VALUES (1, 'ModelTag')", [])
                    .unwrap();
                Ok(())
            })
            .unwrap();

        let weights = vec![0.1, -0.2, 0.5, 1.5];
        let bias = 0.75;

        db.repo
            .save_tag_model_impl(1, &weights, bias, -2.0, 0.0, 0)
            .unwrap();

        db.repo
            .with_conn(|conn| {
                let mut stmt = conn
                    .prepare("SELECT weights, bias FROM tag_models WHERE tag_id = 1")
                    .unwrap();
                let (weights_bytes, loaded_bias): (Vec<u8>, f64) =
                    stmt.query_row([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();

                assert_eq!(loaded_bias, bias);
                assert_eq!(weights_bytes.len(), weights.len() * 8);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_cascading_deletes() {
        let db = TestDb::new("test_cascade");
        let media_id = Uuid::new_v4();

        insert_test_media(&db.repo, media_id);

        db.repo
            .update_media_tags_impl(media_id, vec!["DeleteMe".to_string()])
            .unwrap();

        // 1. Verify association exists
        db.repo
            .with_conn(|conn| {
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM media_tags", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(count, 1);
                Ok(())
            })
            .unwrap();

        // 2. Delete media item
        db.repo.delete_many_impl(&[media_id]).unwrap();

        // 3. Verify association is GONE (via CASCADE)
        db.repo
            .with_conn(|conn| {
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM media_tags", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(count, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_empty_inputs_safety() {
        let db = TestDb::new("test_empty");

        // Should not crash or error on empty batch
        db.repo
            .update_media_tags_batch_impl(&[], &["Tag".to_string()])
            .unwrap();
        db.repo
            .update_media_tags_batch_impl(&[Uuid::new_v4()], &[])
            .unwrap();

        // Auto-tag with empty matches
        db.repo.update_auto_tags_impl(1, &[], None).unwrap();
    }

    #[test]
    fn test_invalid_media_id_tagging() {
        let db = TestDb::new("test_invalid");
        let fake_id = Uuid::new_v4();

        db.repo
            .with_conn(|conn| {
                conn.execute("INSERT INTO tags (name) VALUES ('Nature')", [])
                    .unwrap();
                Ok(())
            })
            .unwrap();
        let tag_id = db.repo.get_tag_id_by_name_impl("Nature").unwrap().unwrap();

        // Attempt to auto-tag an ID that doesn't exist in 'media' table
        // This should fail due to FOREIGN KEY constraint
        let res = db
            .repo
            .update_auto_tags_impl(tag_id, &[(fake_id, 0.9)], None);
        assert!(
            res.is_err(),
            "Should have failed due to foreign key constraint"
        );
    }

    #[test]
    fn test_stale_auto_tag_cleanup() {
        let db = TestDb::new("test_stale");
        let media_id = Uuid::new_v4();

        insert_test_media(&db.repo, media_id);
        db.repo
            .with_conn(|conn| {
                conn.execute("INSERT INTO tags (name) VALUES ('Stale')", [])
                    .unwrap();
                Ok(())
            })
            .unwrap();

        let tag_id = db.repo.get_tag_id_by_name_impl("Stale").unwrap().unwrap();

        // 1. Setup: Item has an auto-tag
        db.repo
            .update_auto_tags_impl(tag_id, &[(media_id, 0.7)], None)
            .unwrap();

        // 2. Verify it exists
        let auto_tags = db.repo.get_tags_with_auto_counts_impl().unwrap();
        assert!(auto_tags.iter().any(|(id, _, _)| *id == tag_id));

        // 3. Cleanup: Call update_auto_tags with empty list for this tag
        db.repo.update_auto_tags_impl(tag_id, &[], None).unwrap();

        // 4. Verify: It should be gone
        let auto_tags_after = db.repo.get_tags_with_auto_counts_impl().unwrap();
        assert!(!auto_tags_after.iter().any(|(id, _, _)| *id == tag_id));
    }

    #[test]
    fn test_count_auto_tags_scoped() {
        let db = TestDb::new("test_scoped_count");
        let media_id = Uuid::new_v4();
        let folder_id = Uuid::new_v4();

        db.repo.with_conn(|conn| {
            // 1. Setup media
            conn.execute(
                "INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date)
                 VALUES (?1, 'test.jpg', 'test.jpg', 100, 'abc', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                params![media_id.as_bytes()],
            ).unwrap();

            // 2. Setup folder and add media
            conn.execute("INSERT INTO folders (id, name, created_at) VALUES (?1, 'Test', '2024-01-01')", params![folder_id.as_bytes()]).unwrap();
            conn.execute("INSERT INTO folder_media (folder_id, media_id, added_at) VALUES (?1, ?2, '2024-01-01')", params![folder_id.as_bytes(), media_id.as_bytes()]).unwrap();

            // 3. Setup tag
            conn.execute("INSERT INTO tags (id, name) VALUES (1, 'Nature')", []).unwrap();
            Ok(())
        }).unwrap();

        // 4. Global count should be 0
        assert_eq!(db.repo.count_auto_tags_impl(None).unwrap(), 0);

        // 5. Add auto-tag
        db.repo
            .update_auto_tags_impl(1, &[(media_id, 0.9)], None)
            .unwrap();

        // 6. Verify counts
        assert_eq!(
            db.repo.count_auto_tags_impl(None).unwrap(),
            1,
            "Global count should be 1"
        );
        assert_eq!(
            db.repo.count_auto_tags_impl(Some(folder_id)).unwrap(),
            1,
            "Folder count should be 1"
        );

        // 7. Verify count for DIFFERENT folder is 0
        let other_folder = Uuid::new_v4();
        db.repo
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO folders (id, name, created_at) VALUES (?1, 'Other', '2024-01-01')",
                    params![other_folder.as_bytes()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(
            db.repo.count_auto_tags_impl(Some(other_folder)).unwrap(),
            0,
            "Other folder count should be 0"
        );
    }
}
