use crate::domain::DomainError;
use rusqlite::params;
use uuid::Uuid;

use super::SqliteRepository;

impl SqliteRepository {
    pub(crate) fn get_all_tags_impl(&self) -> Result<Vec<String>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT t.name FROM tags t
                     JOIN media_tags mt ON mt.tag_id = t.id
                     ORDER BY t.name",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut tags = Vec::new();
            for row in rows {
                tags.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(tags)
        })
    }

    pub(crate) fn update_media_tags_impl(
        &self,
        id: Uuid,
        tags: Vec<String>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Remove existing tags for this media
            conn.execute(
                "DELETE FROM media_tags WHERE media_id = ?1",
                params![id.as_bytes()],
            )
            .map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                DomainError::Database(e.to_string())
            })?;

            // Insert new tags
            for tag_name in &tags {
                let trimmed = tag_name.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Ensure tag exists (INSERT OR IGNORE)
                conn.execute(
                    "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                    params![trimmed],
                )
                .map_err(|e| {
                    let _ = conn.execute("ROLLBACK", []);
                    DomainError::Database(e.to_string())
                })?;

                // Get the tag ID
                let tag_id: i64 = conn
                    .query_row(
                        "SELECT id FROM tags WHERE name = ?1",
                        params![trimmed],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        let _ = conn.execute("ROLLBACK", []);
                        DomainError::Database(e.to_string())
                    })?;

                // Link media to tag
                conn.execute(
                    "INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?1, ?2)",
                    params![id.as_bytes(), tag_id],
                )
                .map_err(|e| {
                    let _ = conn.execute("ROLLBACK", []);
                    DomainError::Database(e.to_string())
                })?;
            }

            conn.execute("COMMIT", [])
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
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Ensure all tags exist first
            let mut tag_ids: Vec<i64> = Vec::with_capacity(tags.len());
            for tag_name in tags {
                let trimmed = tag_name.trim();
                if trimmed.is_empty() {
                    continue;
                }

                conn.execute(
                    "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                    params![trimmed],
                )
                .map_err(|e| {
                    let _ = conn.execute("ROLLBACK", []);
                    DomainError::Database(e.to_string())
                })?;

                let tag_id: i64 = conn
                    .query_row(
                        "SELECT id FROM tags WHERE name = ?1",
                        params![trimmed],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        let _ = conn.execute("ROLLBACK", []);
                        DomainError::Database(e.to_string())
                    })?;

                tag_ids.push(tag_id);
            }

            // For each media item, replace tags
            for id in ids {
                conn.execute(
                    "DELETE FROM media_tags WHERE media_id = ?1",
                    params![id.as_bytes()],
                )
                .map_err(|e| {
                    let _ = conn.execute("ROLLBACK", []);
                    DomainError::Database(e.to_string())
                })?;

                for &tag_id in &tag_ids {
                    conn.execute(
                        "INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?1, ?2)",
                        params![id.as_bytes(), tag_id],
                    )
                    .map_err(|e| {
                        let _ = conn.execute("ROLLBACK", []);
                        DomainError::Database(e.to_string())
                    })?;
                }
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }
}
