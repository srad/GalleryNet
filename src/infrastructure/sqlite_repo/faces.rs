use crate::domain::{DomainError, Face, FaceGroup, MediaSummary};
use rusqlite::params;
use uuid::Uuid;
use super::SqliteRepository;

impl SqliteRepository {
    pub(crate) fn save_faces_impl(
        &self,
        media_id: Uuid,
        faces: &[Face],
        embeddings: &[Vec<f32>],
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Delete existing faces for this media
            let _ = conn.execute(
                "DELETE FROM vec_faces WHERE rowid IN (SELECT rowid FROM faces WHERE media_id = ?1)",
                params![media_id.as_bytes()],
            );
            let _ = conn.execute(
                "DELETE FROM faces WHERE media_id = ?1",
                params![media_id.as_bytes()],
            );

            for (face, embedding) in faces.iter().zip(embeddings.iter()) {
                conn.execute(
                    "INSERT INTO faces (id, media_id, box_x1, box_y1, box_x2, box_y2, cluster_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        face.id.as_bytes(),
                        media_id.as_bytes(),
                        face.box_x1,
                        face.box_y1,
                        face.box_x2,
                        face.box_y2,
                        face.cluster_id,
                    ],
                ).map_err(|e| DomainError::Database(e.to_string()))?;

                let rowid = conn.last_insert_rowid();
                let embedding_bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        embedding.as_ptr() as *const u8,
                        embedding.len() * std::mem::size_of::<f32>(),
                    )
                };

                conn.execute(
                    "INSERT INTO vec_faces (rowid, embedding) VALUES (?1, ?2)",
                    params![rowid, embedding_bytes],
                ).map_err(|e| DomainError::Database(e.to_string()))?;
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_all_face_embeddings_impl(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Vec<f32>)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT f.id, f.media_id, v.embedding
                 FROM faces f
                 JOIN vec_faces v ON v.rowid = f.rowid"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map([], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let media_id_bytes: Vec<u8> = row.get(1)?;
                let embedding_bytes: Vec<u8> = row.get(2)?;

                let id = Uuid::from_slice(&id_bytes).unwrap();
                let media_id = Uuid::from_slice(&media_id_bytes).unwrap();

                let count = embedding_bytes.len() / 4;
                let mut embedding = Vec::with_capacity(count);
                for chunk in embedding_bytes.chunks_exact(4) {
                    let arr: [u8; 4] = chunk.try_into().unwrap();
                    embedding.push(f32::from_ne_bytes(arr));
                }
                crate::infrastructure::sqlite_repo::normalize_vector(&mut embedding);

                Ok((id, media_id, embedding))
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn update_face_clusters_impl(
        &self,
        face_ids_with_clusters: &[(Uuid, i64)],
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut stmt = conn.prepare(
                "UPDATE faces SET cluster_id = ?1 WHERE id = ?2"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            for (id, cluster_id) in face_ids_with_clusters {
                stmt.execute(params![cluster_id, id.as_bytes()])
                    .map_err(|e| DomainError::Database(e.to_string()))?;
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_face_groups_impl(&self) -> Result<Vec<FaceGroup>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT cluster_id, m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (f.media_id IS NOT NULL) as is_favorite, m.size_bytes
                 FROM faces fs
                 JOIN media m ON m.id = fs.media_id
                 LEFT JOIN favorites f ON f.media_id = m.id
                 WHERE cluster_id IS NOT NULL
                 GROUP BY cluster_id, m.id
                 ORDER BY cluster_id, m.original_date DESC"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map([], |row| {
                let cluster_id: i64 = row.get(0)?;
                let id_bytes: Vec<u8> = row.get(1)?;
                let filename: String = row.get(2)?;
                let original_filename: String = row.get(3)?;
                let media_type: String = row.get(4)?;
                let uploaded_at_str: String = row.get(5)?;
                let original_date_str: String = row.get(6)?;
                let is_favorite: bool = row.get(7)?;
                let size_bytes: i64 = row.get(8)?;

                let id = Uuid::from_slice(&id_bytes).unwrap();
                let uploaded_at = chrono::DateTime::parse_from_rfc3339(&uploaded_at_str).unwrap().with_timezone(&chrono::Utc);
                let original_date = chrono::DateTime::parse_from_rfc3339(&original_date_str).unwrap().with_timezone(&chrono::Utc);

                Ok((cluster_id, MediaSummary {
                    id,
                    filename,
                    original_filename,
                    media_type,
                    uploaded_at,
                    original_date,
                    size_bytes,
                    is_favorite,
                    tags: vec![],
                }))
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut groups_map: std::collections::BTreeMap<i64, Vec<MediaSummary>> = std::collections::BTreeMap::new();
            for row in rows {
                let (cluster_id, summary) = row.map_err(|e| DomainError::Database(e.to_string()))?;
                groups_map.entry(cluster_id).or_default().push(summary);
            }

            let mut groups = Vec::new();
            for (cluster_id, items) in groups_map {
                if items.len() < 2 {
                    continue;
                }
                groups.push(FaceGroup { id: cluster_id, items });
            }

            Ok(groups)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::TestDb;
    use crate::domain::{Face, MediaItem};
    use crate::domain::ports::MediaRepository;
    use uuid::Uuid;
    use chrono::Utc;

    #[test]
    fn test_save_and_get_faces() {
        let db = TestDb::new("test_faces");
        let media_id = Uuid::new_v4();
        
        // Insert media first
        let media = MediaItem {
            id: media_id,
            filename: "test.jpg".to_string(),
            original_filename: "test.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "phash".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: Some(100),
            height: Some(100),
            size_bytes: 1000,
            exif_json: None,
            is_favorite: false,
            tags: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        let face_id = Uuid::new_v4();
        let faces = vec![Face {
            id: face_id,
            media_id,
            box_x1: 10, box_y1: 10, box_x2: 50, box_y2: 50,
            cluster_id: None,
        }];
        let embeddings = vec![vec![0.1f32; 512]];

        db.repo.save_faces(media_id, &faces, &embeddings).unwrap();

        let all_embeddings = db.repo.get_all_face_embeddings().unwrap();
        assert_eq!(all_embeddings.len(), 1);
        assert_eq!(all_embeddings[0].0, face_id);
        assert_eq!(all_embeddings[0].1, media_id);
        assert_eq!(all_embeddings[0].2.len(), 512);
    }

    #[test]
    fn test_face_clustering_and_groups() {
        let db = TestDb::new("test_face_groups");
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        
        for id in &[id1, id2] {
            let media = MediaItem {
                id: *id,
                filename: format!("{}.jpg", id),
                original_filename: "test.jpg".to_string(),
                media_type: "image".to_string(),
                phash: id.to_string(),
                uploaded_at: Utc::now(),
                original_date: Utc::now(),
                width: Some(100), height: Some(100), size_bytes: 1000,
                exif_json: None, is_favorite: false, tags: vec![],
            };
            db.repo.save_metadata_and_vector(&media, None).unwrap();
        }

        let face_id1 = Uuid::new_v4();
        let face_id2 = Uuid::new_v4();
        
        // Save face for media 1
        db.repo.save_faces(id1, &[Face { id: face_id1, media_id: id1, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None }], &[vec![0.1; 512]]).unwrap();
        // Save face for media 2
        db.repo.save_faces(id2, &[Face { id: face_id2, media_id: id2, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None }], &[vec![0.1; 512]]).unwrap();

        db.repo.update_face_clusters(&[(face_id1, 100), (face_id2, 100)]).unwrap();

        let groups = db.repo.get_face_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, 100);
        assert_eq!(groups[0].items.len(), 2);
    }

    #[test]
    fn test_face_cascade_delete() {
        let db = TestDb::new("test_cascade");
        let media_id = Uuid::new_v4();
        
        let media = MediaItem {
            id: media_id,
            filename: "test.jpg".to_string(),
            original_filename: "test.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "ph".to_string(),
            uploaded_at: Utc::now(), original_date: Utc::now(),
            width: Some(100), height: Some(100), size_bytes: 1000,
            exif_json: None, is_favorite: false, tags: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();
        
        db.repo.save_faces(media_id, &[Face {
            id: Uuid::new_v4(), media_id, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None
        }], &[vec![0.1; 512]]).unwrap();

        // 1. Verify existence
        assert_eq!(db.repo.get_all_face_embeddings().unwrap().len(), 1);

        // 2. Delete media
        db.repo.delete(media_id).unwrap();

        // 3. Verify faces are gone
        assert_eq!(db.repo.get_all_face_embeddings().unwrap().len(), 0);
    }
}
