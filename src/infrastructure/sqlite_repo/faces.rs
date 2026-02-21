use super::SqliteRepository;
use crate::domain::{DomainError, Face, FaceGroup, FaceStats, MediaItem, MediaSummary, Person};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

impl SqliteRepository {
    pub(crate) fn save_faces_impl(
        &self,
        media_id: Uuid,
        faces: &[Face],
        embeddings: &[Vec<f32>],
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            for (face, embedding) in faces.iter().zip(embeddings.iter()) {
                conn.execute(
                    "INSERT INTO faces (id, media_id, box_x1, box_y1, box_x2, box_y2, cluster_id, person_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        face.id.as_bytes(),
                        media_id.as_bytes(),
                        face.box_x1,
                        face.box_y1,
                        face.box_x2,
                        face.box_y2,
                        face.cluster_id,
                        face.person_id.map(|id| id.as_bytes().to_vec()),
                    ],
                ).map_err(|e| DomainError::Database(e.to_string()))?;

                let rowid: i64 = conn.last_insert_rowid();
                let vector_bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        embedding.as_ptr() as *const u8,
                        embedding.len() * std::mem::size_of::<f32>(),
                    )
                };

                conn.execute(
                    "INSERT INTO vec_faces (rowid, embedding) VALUES (?1, ?2)",
                    params![rowid, vector_bytes],
                ).map_err(|e| DomainError::Database(e.to_string()))?;
            }
            Ok(())
        })
    }

    pub(crate) fn get_all_face_embeddings_impl(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Vec<f32>)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT f.id, f.media_id, v.embedding
                  FROM faces f
                  JOIN vec_faces v ON v.rowid = f.rowid",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map([], |row| {
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
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn get_face_embedding_impl(&self, id: Uuid) -> Result<Vec<f32>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT v.embedding FROM vec_faces v JOIN faces f ON f.rowid = v.rowid WHERE f.id = ?1",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let bytes: Vec<u8> = stmt.query_row(params![id.as_bytes()], |row| row.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let mut vector = Vec::with_capacity(bytes.len() / 4);
            for chunk in bytes.chunks_exact(4) {
                let arr: [u8; 4] = chunk.try_into().unwrap();
                vector.push(f32::from_ne_bytes(arr));
            }
            Ok(vector)
        })
    }

    pub(crate) fn get_nearest_face_embeddings_impl(
        &self,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<(Uuid, Uuid, f32)>, DomainError> {
        self.with_conn(|conn| {
            let vector_bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    vector.as_ptr() as *const u8,
                    vector.len() * std::mem::size_of::<f32>(),
                )
            };

            let mut stmt = conn
                .prepare(
                    "SELECT f.id, f.media_id, v.distance
                 FROM (
                    SELECT rowid, distance
                    FROM vec_faces
                    WHERE embedding MATCH ?1
                    ORDER BY distance
                    LIMIT ?2
                 ) v
                 JOIN faces f ON f.rowid = v.rowid",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![vector_bytes, limit as i64], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let media_id_bytes: Vec<u8> = row.get(1)?;
                    let distance: f32 = row.get(2)?;

                    let id = Uuid::from_slice(&id_bytes).unwrap();
                    let media_id = Uuid::from_slice(&media_id_bytes).unwrap();

                    Ok((id, media_id, distance))
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

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
            let tx = conn
                .transaction()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            {
                let mut stmt = tx
                    .prepare("UPDATE faces SET cluster_id = ?1 WHERE id = ?2")
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                for (id, cluster_id) in face_ids_with_clusters {
                    stmt.execute(params![cluster_id, id.as_bytes()])
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                }
            }
            tx.commit()
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_face_groups_impl(&self) -> Result<Vec<FaceGroup>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT cluster_id, m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (f.media_id IS NOT NULL) as is_favorite, m.size_bytes, m.width, m.height
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
                let width: Option<u32> = row.get(9)?;
                let height: Option<u32> = row.get(10)?;

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
                    width,
                    height,
                    size_bytes,
                    is_favorite,
                    tags: vec![],
                }))
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut groups_map: std::collections::HashMap<i64, Vec<MediaSummary>> = std::collections::HashMap::new();
            for row in rows {
                let (cluster_id, item) = row.map_err(|e| DomainError::Database(e.to_string()))?;
                groups_map.entry(cluster_id).or_default().push(item);
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

    pub(crate) fn get_cluster_representatives_impl(
        &self,
    ) -> Result<Vec<(i64, MediaItem, Face)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT f.cluster_id, m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, (fav.media_id IS NOT NULL) as is_favorite,
                        f.id, f.media_id, f.box_x1, f.box_y1, f.box_x2, f.box_y2, f.cluster_id, f.person_id
                 FROM faces f
                 JOIN media m ON m.id = f.media_id
                 LEFT JOIN favorites fav ON fav.media_id = m.id
                 WHERE f.cluster_id IS NOT NULL
                 GROUP BY f.cluster_id
                 ORDER BY f.cluster_id"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map([], |row| {
                let cluster_id: i64 = row.get(0)?;
                
                // MediaItem
                let m_id_bytes: Vec<u8> = row.get(1)?;
                let filename: String = row.get(2)?;
                let original_filename: String = row.get(3)?;
                let media_type: String = row.get(4)?;
                let phash: String = row.get(5)?;
                let uploaded_at_str: String = row.get(6)?;
                let original_date_str: String = row.get(7)?;
                let width: Option<u32> = row.get(8)?;
                let height: Option<u32> = row.get(9)?;
                let size_bytes: i64 = row.get(10)?;
                let exif_json: Option<String> = row.get(11)?;
                let is_favorite: bool = row.get(12)?;

                let m_id = Uuid::from_slice(&m_id_bytes).unwrap();
                let uploaded_at = DateTime::parse_from_rfc3339(&uploaded_at_str).unwrap().with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str).unwrap().with_timezone(&Utc);

                let media = MediaItem {
                    id: m_id,
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
                    faces_scanned: true,
                    tags: vec![],
                    faces: vec![],
                };

                // Face
                let f_id_bytes: Vec<u8> = row.get(13)?;
                let f_media_id_bytes: Vec<u8> = row.get(14)?;
                let box_x1: i32 = row.get(15)?;
                let box_y1: i32 = row.get(16)?;
                let box_x2: i32 = row.get(17)?;
                let box_y2: i32 = row.get(18)?;
                let f_cluster_id: Option<i64> = row.get(19)?;
                let person_id_bytes: Option<Vec<u8>> = row.get(20)?;

                let f_id = Uuid::from_slice(&f_id_bytes).unwrap();
                let f_media_id = Uuid::from_slice(&f_media_id_bytes).unwrap();
                let person_id = person_id_bytes.map(|b| Uuid::from_slice(&b).unwrap());

                let face = Face {
                    id: f_id,
                    media_id: f_media_id,
                    box_x1,
                    box_y1,
                    box_x2,
                    box_y2,
                    cluster_id: f_cluster_id,
                    person_id,
                };

                Ok((cluster_id, media, face))
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn find_media_unscanned_faces_impl(
        &self,
        limit: usize,
    ) -> Result<Vec<MediaItem>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, (fav.media_id IS NOT NULL) as is_favorite
                 FROM media m
                 LEFT JOIN favorites fav ON fav.media_id = m.id
                 WHERE m.faces_scanned = 0
                 ORDER BY m.original_date DESC
                 LIMIT ?1"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map(params![limit as i64], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let id = Uuid::from_slice(&id_bytes).unwrap();
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let phash: String = row.get(4)?;
                let uploaded_at_str: String = row.get(5)?;
                let original_date_str: String = row.get(6)?;
                let width: Option<u32> = row.get(7)?;
                let height: Option<u32> = row.get(8)?;
                let size_bytes: i64 = row.get(9)?;
                let exif_json: Option<String> = row.get(10)?;
                let is_favorite: bool = row.get(11)?;

                let uploaded_at = DateTime::parse_from_rfc3339(&uploaded_at_str).unwrap().with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str).unwrap().with_timezone(&Utc);

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
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn set_faces_scanned_impl(
        &self,
        media_id: Uuid,
        scanned: bool,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE media SET faces_scanned = ?1 WHERE id = ?2",
                params![scanned, media_id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn save_face_indexing_results_impl(
        &self,
        media_id: Uuid,
        faces: &[Face],
        embeddings: &[Vec<f32>],
    ) -> Result<(), DomainError> {
        self.save_faces_impl(media_id, faces, embeddings)?;
        self.set_faces_scanned_impl(media_id, true)
    }

    pub(crate) fn find_media_missing_embeddings_impl(&self) -> Result<Vec<MediaItem>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, (fav.media_id IS NOT NULL) as is_favorite
                 FROM media m
                 LEFT JOIN favorites fav ON fav.media_id = m.id
                 LEFT JOIN vec_media v ON v.rowid = m.rowid
                 WHERE v.embedding IS NULL AND m.phash != 'no_hash'"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map([], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let id = Uuid::from_slice(&id_bytes).unwrap();
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let phash: String = row.get(4)?;
                let uploaded_at_str: String = row.get(5)?;
                let original_date_str: String = row.get(6)?;
                let width: Option<u32> = row.get(7)?;
                let height: Option<u32> = row.get(8)?;
                let size_bytes: i64 = row.get(9)?;
                let exif_json: Option<String> = row.get(10)?;
                let is_favorite: bool = row.get(11)?;

                let uploaded_at = DateTime::parse_from_rfc3339(&uploaded_at_str).unwrap().with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str).unwrap().with_timezone(&Utc);

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
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn get_media_items_by_ids_impl(
        &self,
        ids: &[Uuid],
    ) -> Result<Vec<MediaItem>, DomainError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        self.with_conn(|conn| {
            let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
            let sql = format!(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.phash, m.uploaded_at, m.original_date, m.width, m.height, m.size_bytes, m.exif_json, (fav.media_id IS NOT NULL) as is_favorite, m.faces_scanned
                 FROM media m
                 LEFT JOIN favorites fav ON fav.media_id = m.id
                 WHERE m.id IN ({})",
                placeholders.join(", ")
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| DomainError::Database(e.to_string()))?;
            let params_vec: Vec<Vec<u8>> = ids.iter().map(|id| id.as_bytes().to_vec()).collect();
            let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let id = Uuid::from_slice(&id_bytes).unwrap();
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let phash: String = row.get(4)?;
                let uploaded_at_str: String = row.get(5)?;
                let original_date_str: String = row.get(6)?;
                let width: Option<u32> = row.get(7)?;
                let height: Option<u32> = row.get(8)?;
                let size_bytes: i64 = row.get(9)?;
                let exif_json: Option<String> = row.get(10)?;
                let is_favorite: bool = row.get(11)?;
                let faces_scanned: bool = row.get(12)?;

                let uploaded_at = DateTime::parse_from_rfc3339(&uploaded_at_str).unwrap().with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str).unwrap().with_timezone(&Utc);

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
                    faces_scanned,
                    tags: vec![],
                    faces: vec![],
                })
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn reset_face_index_impl(&self) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM vec_faces", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            conn.execute("DELETE FROM faces", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            conn.execute("UPDATE media SET faces_scanned = 0", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn list_people_impl(
        &self,
        include_hidden: bool,
    ) -> Result<Vec<(Person, Option<Face>, Option<MediaSummary>)>, DomainError> {
        self.with_conn(|conn| {
            let sql = format!(
                "SELECT p.id, p.name, p.is_hidden, 
                        (SELECT COUNT(*) FROM faces WHERE person_id = p.id) as face_count,
                        f.id, f.media_id, f.box_x1, f.box_y1, f.box_x2, f.box_y2, f.cluster_id, f.person_id,
                        m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, m.size_bytes, m.width, m.height
                 FROM people p
                 LEFT JOIN faces f ON f.id = COALESCE(p.representative_face_id, (SELECT id FROM faces WHERE person_id = p.id LIMIT 1))
                 LEFT JOIN media m ON m.id = f.media_id
                 {}
                 ORDER BY p.name",
                if include_hidden { "" } else { "WHERE p.is_hidden = 0" }
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| DomainError::Database(e.to_string()))?;
            let rows = stmt.query_map([], |row| {
                let p_id_bytes: Vec<u8> = row.get(0)?;
                let p_id = Uuid::from_slice(&p_id_bytes).unwrap();
                let name: Option<String> = row.get(1)?;
                let name = name.unwrap_or_else(|| "Unnamed".to_string());
                let is_hidden: bool = row.get(2)?;
                let face_count: i64 = row.get(3)?;
                let person = Person { id: p_id, name, is_hidden, face_count, representative_face_id: None };

                let face = match row.get::<_, Option<Vec<u8>>>(4)? {
                    Some(f_id_bytes) => {
                        let f_id = Uuid::from_slice(&f_id_bytes).unwrap();
                        let f_media_id_bytes: Vec<u8> = row.get(5)?;
                        let f_media_id = Uuid::from_slice(&f_media_id_bytes).unwrap();
                        let person_id_bytes: Option<Vec<u8>> = row.get(11)?;
                        Some(Face {
                            id: f_id,
                            media_id: f_media_id,
                            box_x1: row.get(6)?,
                            box_y1: row.get(7)?,
                            box_x2: row.get(8)?,
                            box_y2: row.get(9)?,
                            cluster_id: row.get(10)?,
                            person_id: person_id_bytes.map(|b| Uuid::from_slice(&b).unwrap()),
                        })
                    }
                    None => None,
                };

                let media = match row.get::<_, Option<Vec<u8>>>(12)? {
                    Some(m_id_bytes) => {
                        let m_id = Uuid::from_slice(&m_id_bytes).unwrap();
                        let uploaded_at = DateTime::parse_from_rfc3339(&row.get::<_, String>(16)?).unwrap().with_timezone(&Utc);
                        let original_date = DateTime::parse_from_rfc3339(&row.get::<_, String>(17)?).unwrap().with_timezone(&Utc);
                        Some(MediaSummary {
                            id: m_id,
                            filename: row.get(13)?,
                            original_filename: row.get(14)?,
                            media_type: row.get(15)?,
                            uploaded_at,
                            original_date,
                            width: row.get(19)?,
                            height: row.get(20)?,
                            size_bytes: row.get(18)?,
                            is_favorite: false,
                            tags: vec![],
                        })
                    }
                    None => None,
                };

                Ok((person, face, media))
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn get_person_impl(
        &self,
        id: Uuid,
    ) -> Result<Option<(Person, Option<Face>, Option<MediaSummary>)>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT p.id, p.name, p.is_hidden, 
                        (SELECT COUNT(*) FROM faces WHERE person_id = p.id) as face_count,
                        f.id, f.media_id, f.box_x1, f.box_y1, f.box_x2, f.box_y2, f.cluster_id, f.person_id,
                        m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, m.size_bytes, m.width, m.height
                 FROM people p
                 LEFT JOIN faces f ON f.id = COALESCE(p.representative_face_id, (SELECT id FROM faces WHERE person_id = p.id LIMIT 1))
                 LEFT JOIN media m ON m.id = f.media_id
                 WHERE p.id = ?1"
            ).map_err(|e| DomainError::Database(e.to_string()))?;
            
            let result = stmt.query_row(params![id.as_bytes()], |row| {
                let p_id_bytes: Vec<u8> = row.get(0)?;
                let p_id = Uuid::from_slice(&p_id_bytes).unwrap();
                let name: Option<String> = row.get(1)?;
                let name = name.unwrap_or_else(|| "Unnamed".to_string());
                let is_hidden: bool = row.get(2)?;
                let face_count: i64 = row.get(3)?;
                let person = Person { id: p_id, name, is_hidden, face_count, representative_face_id: None };

                let face = match row.get::<_, Option<Vec<u8>>>(4)? {
                    Some(f_id_bytes) => {
                        let f_id = Uuid::from_slice(&f_id_bytes).unwrap();
                        let f_media_id_bytes: Vec<u8> = row.get(5)?;
                        let f_media_id = Uuid::from_slice(&f_media_id_bytes).unwrap();
                        let person_id_bytes: Option<Vec<u8>> = row.get(11)?;
                        Some(Face {
                            id: f_id,
                            media_id: f_media_id,
                            box_x1: row.get(6)?,
                            box_y1: row.get(7)?,
                            box_x2: row.get(8)?,
                            box_y2: row.get(9)?,
                            cluster_id: row.get(10)?,
                            person_id: person_id_bytes.map(|b| Uuid::from_slice(&b).unwrap()),
                        })
                    }
                    None => None,
                };

                let media = match row.get::<_, Option<Vec<u8>>>(12)? {
                    Some(m_id_bytes) => {
                        let m_id = Uuid::from_slice(&m_id_bytes).unwrap();
                        let uploaded_at = DateTime::parse_from_rfc3339(&row.get::<_, String>(16)?).unwrap().with_timezone(&Utc);
                        let original_date = DateTime::parse_from_rfc3339(&row.get::<_, String>(17)?).unwrap().with_timezone(&Utc);
                        Some(MediaSummary {
                            id: m_id,
                            filename: row.get(13)?,
                            original_filename: row.get(14)?,
                            media_type: row.get(15)?,
                            uploaded_at,
                            original_date,
                            width: row.get(19)?,
                            height: row.get(20)?,
                            size_bytes: row.get(18)?,
                            is_favorite: false,
                            tags: vec![],
                        })
                    }
                    None => None,
                };

                Ok((person, face, media))
            });
            match result {
                Ok(p) => Ok(Some(p)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    pub(crate) fn create_person_impl(&self, id: Uuid, name: &str) -> Result<Person, DomainError> {
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO people (id, name, is_hidden, created_at) VALUES (?1, ?2, 0, ?3)",
                params![id.as_bytes(), name, now],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(Person {
                id,
                name: name.to_string(),
                is_hidden: false,
                face_count: 0,
                representative_face_id: None,
            })
        })
    }

    pub(crate) fn update_person_impl(&self, person: &Person) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE people SET name = ?1, is_hidden = ?2, representative_face_id = ?3 WHERE id = ?4",
                params![
                    person.name,
                    person.is_hidden,
                    person.representative_face_id.map(|id| id.as_bytes().to_vec()),
                    person.id.as_bytes()
                ],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn delete_person_impl(&self, id: Uuid) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM people WHERE id = ?1", params![id.as_bytes()])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn rename_person_impl(&self, id: Uuid, name: &str) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE people SET name = ?1 WHERE id = ?2",
                params![name, id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn name_face_impl(
        &self,
        face_id: Uuid,
        person_id: Option<Uuid>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE faces SET person_id = ?1 WHERE id = ?2",
                params![
                    person_id.map(|id| id.as_bytes().to_vec()),
                    face_id.as_bytes()
                ],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            if let Some(pid) = person_id {
                // Check if person has representative face
                let has_rep: bool = conn
                    .query_row(
                        "SELECT 1 FROM people WHERE id = ?1 AND representative_face_id IS NOT NULL",
                        params![pid.as_bytes()],
                        |_| Ok(true),
                    )
                    .unwrap_or(false);

                if !has_rep {
                    conn.execute(
                        "UPDATE people SET representative_face_id = ?1 WHERE id = ?2",
                        params![face_id.as_bytes(), pid.as_bytes()],
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                }
            }

            Ok(())
        })
    }

    pub(crate) fn name_cluster_impl(
        &self,
        cluster_id: i64,
        person_id: Option<Uuid>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE faces SET person_id = ?1 WHERE cluster_id = ?2",
                params![person_id.map(|id| id.as_bytes().to_vec()), cluster_id],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            if let Some(pid) = person_id {
                let has_rep: bool = conn
                    .query_row(
                        "SELECT 1 FROM people WHERE id = ?1 AND representative_face_id IS NOT NULL",
                        params![pid.as_bytes()],
                        |_| Ok(true),
                    )
                    .unwrap_or(false);

                if !has_rep {
                    // Pick the first face in this cluster
                    let face_id_bytes: Option<Vec<u8>> = conn
                        .query_row(
                            "SELECT id FROM faces WHERE cluster_id = ?1 LIMIT 1",
                            params![cluster_id],
                            |row| row.get(0),
                        )
                        .ok();

                    if let Some(fid) = face_id_bytes {
                        conn.execute(
                            "UPDATE people SET representative_face_id = ?1 WHERE id = ?2",
                            params![fid, pid.as_bytes()],
                        )
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    }
                }
            }

            Ok(())
        })
    }

    pub(crate) fn merge_people_impl(
        &self,
        source_id: Uuid,
        target_id: Uuid,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Move all faces from source to target
            conn.execute(
                "UPDATE faces SET person_id = ?1 WHERE person_id = ?2",
                params![target_id.as_bytes(), source_id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            // Check if target needs a representative face
            let has_rep: bool = conn
                .query_row(
                    "SELECT 1 FROM people WHERE id = ?1 AND representative_face_id IS NOT NULL",
                    params![target_id.as_bytes()],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if !has_rep {
                // Try to get representative face from source
                let source_rep: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT representative_face_id FROM people WHERE id = ?1",
                        params![source_id.as_bytes()],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();

                if let Some(rep_id) = source_rep {
                    conn.execute(
                        "UPDATE people SET representative_face_id = ?1 WHERE id = ?2",
                        params![rep_id, target_id.as_bytes()],
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                } else {
                    // Fallback to any face of the target (including newly moved ones)
                    let face_id: Option<Vec<u8>> = conn
                        .query_row(
                            "SELECT id FROM faces WHERE person_id = ?1 LIMIT 1",
                            params![target_id.as_bytes()],
                            |row| row.get(0),
                        )
                        .ok();

                    if let Some(fid) = face_id {
                        conn.execute(
                            "UPDATE people SET representative_face_id = ?1 WHERE id = ?2",
                            params![fid, target_id.as_bytes()],
                        )
                        .map_err(|e| DomainError::Database(e.to_string()))?;
                    }
                }
            }

            // Delete source person
            conn.execute(
                "DELETE FROM people WHERE id = ?1",
                params![source_id.as_bytes()],
            )
            .map_err(|e| DomainError::Database(e.to_string()))?;

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    pub(crate) fn get_person_photos_impl(
        &self,
        person_id: Uuid,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (fav.media_id IS NOT NULL) as is_favorite, m.size_bytes, m.width, m.height
                 FROM media m
                 JOIN faces f ON f.media_id = m.id
                 LEFT JOIN favorites fav ON fav.media_id = m.id
                 WHERE f.person_id = ?1
                 ORDER BY m.original_date DESC"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map(params![person_id.as_bytes()], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let id = Uuid::from_slice(&id_bytes).unwrap();
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let uploaded_at_str: String = row.get(4)?;
                let original_date_str: String = row.get(5)?;
                let is_favorite: bool = row.get(6)?;
                let size_bytes: i64 = row.get(7)?;
                let width: Option<u32> = row.get(8)?;
                let height: Option<u32> = row.get(9)?;

                let uploaded_at = DateTime::parse_from_rfc3339(&uploaded_at_str).unwrap().with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str).unwrap().with_timezone(&Utc);

                Ok(MediaSummary {
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
                })
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }

    pub(crate) fn assign_people_to_clusters_impl(&self) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // 1. Propagate existing person_id within clusters
            let propagated = conn
                .execute(
                    "UPDATE faces 
                 SET person_id = (
                     SELECT f2.person_id 
                     FROM faces f2 
                     WHERE f2.cluster_id = faces.cluster_id 
                     AND f2.person_id IS NOT NULL 
                     LIMIT 1
                 )
                 WHERE cluster_id IS NOT NULL 
                 AND person_id IS NULL 
                 AND EXISTS (
                     SELECT 1 
                     FROM faces f3 
                     WHERE f3.cluster_id = faces.cluster_id 
                     AND f3.person_id IS NOT NULL
                 )",
                    [],
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            if propagated > 0 {
                tracing::info!(
                    "Propagated person ID to {} orphan faces within existing clusters.",
                    propagated
                );
            }

            // 2. Identify clusters that are fully orphan (no person_id in any face)
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT cluster_id 
                 FROM faces 
                 WHERE cluster_id IS NOT NULL 
                 GROUP BY cluster_id 
                 HAVING COUNT(person_id) = 0",
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let clusters: Vec<i64> = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();

            let mut created = 0;
            let now = Utc::now().to_rfc3339();

            for cluster_id in clusters {
                let person_id = Uuid::new_v4();
                // Create person
                conn.execute(
                    "INSERT INTO people (id, name, is_hidden, created_at) VALUES (?1, NULL, 0, ?2)",
                    params![person_id.as_bytes(), now],
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

                // Assign to cluster
                let faces_updated = conn
                    .execute(
                        "UPDATE faces SET person_id = ?1 WHERE cluster_id = ?2",
                        params![person_id.as_bytes(), cluster_id],
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;

                // Set representative face
                let face_id: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT id FROM faces WHERE cluster_id = ?1 LIMIT 1",
                        params![cluster_id],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(fid) = face_id {
                    conn.execute(
                        "UPDATE people SET representative_face_id = ?1 WHERE id = ?2",
                        params![fid, person_id.as_bytes()],
                    )
                    .map_err(|e| DomainError::Database(e.to_string()))?;
                }

                tracing::info!(
                    "Created new person for cluster {} with {} faces.",
                    cluster_id,
                    faces_updated
                );
                created += 1;
            }

            conn.execute("COMMIT", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            if created > 0 {
                tracing::info!("Created {} new people from orphan clusters.", created);
            }

            Ok(propagated + created)
        })
    }

    pub(crate) fn get_face_stats_impl(&self) -> Result<FaceStats, DomainError> {
        self.with_conn(|conn| {
            let total_faces: i64 = conn
                .query_row("SELECT COUNT(*) FROM faces", [], |r| r.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let total_people: i64 = conn
                .query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let named_people: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM people WHERE name IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let hidden_people: i64 = conn
                .query_row("SELECT COUNT(*) FROM people WHERE is_hidden = 1", [], |r| {
                    r.get(0)
                })
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let unassigned_faces: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM faces WHERE person_id IS NULL",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let ungrouped_faces: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM faces WHERE cluster_id IS NULL",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| DomainError::Database(e.to_string()))?;

            Ok(FaceStats {
                total_faces,
                total_people,
                named_people,
                hidden_people,
                unassigned_faces,
                ungrouped_faces,
            })
        })
    }

    pub(crate) fn get_unscanned_media_ids_impl(
        &self,
        limit: usize,
    ) -> Result<Vec<Uuid>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id FROM media WHERE faces_scanned = 0 LIMIT ?1")
                .map_err(|e| DomainError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    let bytes: Vec<u8> = row.get(0)?;
                    Ok(Uuid::from_slice(&bytes).unwrap())
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

pub(crate) fn load_faces_for_media(conn: &Connection, media_id: &[u8]) -> Vec<Face> {
    let mut stmt = match conn.prepare(
        "SELECT id, media_id, box_x1, box_y1, box_x2, box_y2, cluster_id, person_id FROM faces WHERE media_id = ?1"
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map(params![media_id], |row| {
        let id_bytes: Vec<u8> = row.get(0)?;
        let media_id_bytes: Vec<u8> = row.get(1)?;
        let person_id_bytes: Option<Vec<u8>> = row.get(7)?;

        Ok(Face {
            id: Uuid::from_slice(&id_bytes).unwrap(),
            media_id: Uuid::from_slice(&media_id_bytes).unwrap(),
            box_x1: row.get(2)?,
            box_y1: row.get(3)?,
            box_x2: row.get(4)?,
            box_y2: row.get(5)?,
            cluster_id: row.get(6)?,
            person_id: person_id_bytes.map(|b| Uuid::from_slice(&b).unwrap()),
        })
    }) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::super::TestDb;
    use crate::domain::ports::MediaRepository;
    use crate::domain::{Face, MediaItem, Person};
    use chrono::Utc;
    use rusqlite::params;
    use uuid::Uuid;

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
            faces_scanned: false,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        let face_id = Uuid::new_v4();
        let faces = vec![Face {
            id: face_id,
            media_id,
            box_x1: 10,
            box_y1: 10,
            box_x2: 50,
            box_y2: 50,
            cluster_id: None,
            person_id: None,
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
                width: Some(100),
                height: Some(100),
                size_bytes: 1000,
                exif_json: None,
                is_favorite: false,
                faces_scanned: false,
                tags: vec![],
                faces: vec![],
            };

            db.repo.save_metadata_and_vector(&media, None).unwrap();
        }

        let face_id1 = Uuid::new_v4();
        let face_id2 = Uuid::new_v4();

        // Save face for media 1
        db.repo
            .save_faces(
                id1,
                &[Face {
                    id: face_id1,
                    media_id: id1,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 10,
                    box_y2: 10,
                    cluster_id: None,
                    person_id: None,
                }],
                &[vec![0.1; 512]],
            )
            .unwrap();
        // Save face for media 2
        db.repo
            .save_faces(
                id2,
                &[Face {
                    id: face_id2,
                    media_id: id2,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 10,
                    box_y2: 10,
                    cluster_id: None,
                    person_id: None,
                }],
                &[vec![0.1; 512]],
            )
            .unwrap();

        db.repo
            .update_face_clusters(&[(face_id1, 100), (face_id2, 100)])
            .unwrap();

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
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: Some(100),
            height: Some(100),
            size_bytes: 1000,
            exif_json: None,
            is_favorite: false,
            faces_scanned: false,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        db.repo
            .save_faces(
                media_id,
                &[Face {
                    id: Uuid::new_v4(),
                    media_id,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 10,
                    box_y2: 10,
                    cluster_id: None,
                    person_id: None,
                }],
                &[vec![0.1; 512]],
            )
            .unwrap();

        // 1. Verify existence
        assert_eq!(db.repo.get_all_face_embeddings().unwrap().len(), 1);

        // 2. Delete media
        db.repo.delete(media_id).unwrap();

        // 3. Verify faces are gone
        assert_eq!(db.repo.get_all_face_embeddings().unwrap().len(), 0);
    }

    #[test]
    fn test_person_management() {
        let db = TestDb::new("test_person");
        let id = Uuid::new_v4();

        // 1. Create
        let person = db.repo.create_person_impl(id, "Alice").unwrap();
        assert_eq!(person.name, "Alice");
        assert_eq!(person.face_count, 0);

        // 2. Get
        let found = db.repo.get_person_impl(id).unwrap().unwrap();
        assert_eq!(found.0.name, "Alice");

        // 3. List
        let people = db.repo.list_people_impl(false).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].0.name, "Alice");

        // 4. Rename
        db.repo.rename_person_impl(id, "Alice Cooper").unwrap();
        let found = db.repo.get_person_impl(id).unwrap().unwrap();
        assert_eq!(found.0.name, "Alice Cooper");

        // 5. Update (hide)
        let mut p = found.0;
        p.is_hidden = true;
        db.repo.update_person_impl(&p).unwrap();

        let people = db.repo.list_people_impl(false).unwrap();
        assert_eq!(people.len(), 0);
        let people = db.repo.list_people_impl(true).unwrap();
        assert_eq!(people.len(), 1);

        // 6. Delete
        db.repo.delete_person_impl(id).unwrap();
        assert!(db.repo.get_person_impl(id).unwrap().is_none());
    }

    #[test]
    fn test_name_face_and_cluster() {
        let db = TestDb::new("test_naming");
        let media_id = Uuid::new_v4();
        let person_id = Uuid::new_v4();

        // Setup media and face
        let media = MediaItem {
            id: media_id,
            filename: "test.jpg".to_string(),
            original_filename: "test.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: true,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();
        db.repo.create_person_impl(person_id, "Bob").unwrap();

        let face_id = Uuid::new_v4();
        db.repo
            .save_faces(
                media_id,
                &[Face {
                    id: face_id,
                    media_id,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 1,
                    box_y2: 1,
                    cluster_id: Some(5),
                    person_id: None,
                }],
                &[vec![0.0; 512]],
            )
            .unwrap();

        // 1. Name single face
        db.repo.name_face_impl(face_id, Some(person_id)).unwrap();
        let person = db.repo.get_person_impl(person_id).unwrap().unwrap();
        assert_eq!(person.0.face_count, 1);

        // 2. Name cluster
        db.repo.name_cluster_impl(5, Some(person_id)).unwrap();
        // still 1 because it's the same face
        let person = db.repo.get_person_impl(person_id).unwrap().unwrap();
        assert_eq!(person.0.face_count, 1);
    }

    #[test]
    fn test_merge_people() {
        let db = TestDb::new("test_merge");
        let p1_id = Uuid::new_v4();
        let p2_id = Uuid::new_v4();
        db.repo.create_person_impl(p1_id, "P1").unwrap();
        db.repo.create_person_impl(p2_id, "P2").unwrap();

        let media_id = Uuid::new_v4();
        let media = MediaItem {
            id: media_id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: true,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        let f1 = Uuid::new_v4();
        let f2 = Uuid::new_v4();
        db.repo
            .save_faces(
                media_id,
                &[
                    Face {
                        id: f1,
                        media_id,
                        box_x1: 0,
                        box_y1: 0,
                        box_x2: 1,
                        box_y2: 1,
                        cluster_id: None,
                        person_id: Some(p1_id),
                    },
                    Face {
                        id: f2,
                        media_id,
                        box_x1: 1,
                        box_y1: 1,
                        box_x2: 2,
                        box_y2: 2,
                        cluster_id: None,
                        person_id: Some(p2_id),
                    },
                ],
                &[vec![0.0; 512], vec![0.0; 512]],
            )
            .unwrap();

        assert_eq!(
            db.repo
                .get_person_impl(p1_id)
                .unwrap()
                .unwrap()
                .0
                .face_count,
            1
        );
        assert_eq!(
            db.repo
                .get_person_impl(p2_id)
                .unwrap()
                .unwrap()
                .0
                .face_count,
            1
        );

        // Merge P1 into P2
        db.repo.merge_people_impl(p1_id, p2_id).unwrap();

        assert!(db.repo.get_person_impl(p1_id).unwrap().is_none());
        assert_eq!(
            db.repo
                .get_person_impl(p2_id)
                .unwrap()
                .unwrap()
                .0
                .face_count,
            2
        );
    }

    #[test]
    fn test_unscanned_media_tracking() {
        let db = TestDb::new("test_unscanned");
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        for (id, scanned) in [(id1, true), (id2, false)] {
            let media = MediaItem {
                id,
                filename: format!("{}.jpg", id),
                original_filename: "t.jpg".to_string(),
                media_type: "image".to_string(),
                phash: id.to_string(),
                uploaded_at: Utc::now(),
                original_date: Utc::now(),
                width: None,
                height: None,
                size_bytes: 10,
                exif_json: None,
                is_favorite: false,
                faces_scanned: scanned,
                tags: vec![],
                faces: vec![],
            };
            db.repo.save_metadata_and_vector(&media, None).unwrap();
            if scanned {
                db.repo.set_faces_scanned_impl(id, true).unwrap();
            }
        }

        let unscanned = db.repo.get_unscanned_media_ids_impl(10).unwrap();
        assert_eq!(unscanned.len(), 1);
        assert_eq!(unscanned[0], id2);

        db.repo.set_faces_scanned_impl(id2, true).unwrap();
        let unscanned = db.repo.get_unscanned_media_ids_impl(10).unwrap();
        assert_eq!(unscanned.len(), 0);
    }

    #[test]
    fn test_get_person_photos() {
        let db = TestDb::new("test_person_photos");
        let p_id = Uuid::new_v4();
        db.repo.create_person_impl(p_id, "Alice").unwrap();

        let m_id = Uuid::new_v4();
        let media = MediaItem {
            id: m_id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: true,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        db.repo
            .save_faces(
                m_id,
                &[Face {
                    id: Uuid::new_v4(),
                    media_id: m_id,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 1,
                    box_y2: 1,
                    cluster_id: None,
                    person_id: Some(p_id),
                }],
                &[vec![0.0; 512]],
            )
            .unwrap();

        let photos = db.repo.get_person_photos_impl(p_id).unwrap();
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].id, m_id);
    }

    #[test]
    fn test_person_with_null_name() {
        let db = TestDb::new("test_null_name");
        let id = Uuid::new_v4();

        // Manually insert a person with NULL name
        db.repo
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO people (id, name, is_hidden, created_at) VALUES (?1, NULL, 0, ?2)",
                    params![id.as_bytes(), Utc::now().to_rfc3339()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();

        // Verify get_person handles it
        let person = db.repo.get_person_impl(id).unwrap().unwrap();
        assert_eq!(person.0.name, "Unnamed");

        // Verify list_people handles it
        let list = db.repo.list_people_impl(false).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0.name, "Unnamed");

        // Verify stats handle it
        let stats = db.repo.get_face_stats_impl().unwrap();
        assert_eq!(stats.total_people, 1);
        assert_eq!(stats.named_people, 0);
    }

    #[test]
    fn test_face_indexing_flow() {
        let db = TestDb::new("test_indexing");
        let id = Uuid::new_v4();
        let media = MediaItem {
            id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: false,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        // 1. Find unscanned
        let unscanned = db.repo.find_media_unscanned_faces_impl(10).unwrap();
        assert_eq!(unscanned.len(), 1);
        assert_eq!(unscanned[0].id, id);

        // 2. Save results
        let faces = vec![Face {
            id: Uuid::new_v4(),
            media_id: id,
            box_x1: 0,
            box_y1: 0,
            box_x2: 1,
            box_y2: 1,
            cluster_id: None,
            person_id: None,
        }];
        db.repo
            .save_face_indexing_results_impl(id, &faces, &[vec![0.0; 512]])
            .unwrap();

        // 3. Verify scanned
        let unscanned = db.repo.find_media_unscanned_faces_impl(10).unwrap();
        assert_eq!(unscanned.len(), 0);
    }

    #[test]
    fn test_missing_embeddings_and_ids() {
        let db = TestDb::new("test_missing");
        let id = Uuid::new_v4();
        let media = MediaItem {
            id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: false,
            tags: vec![],
            faces: vec![],
        };
        // Save without embedding
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        let missing = db.repo.find_media_missing_embeddings_impl().unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].id, id);

        let by_ids = db.repo.get_media_items_by_ids_impl(&[id]).unwrap();
        assert_eq!(by_ids.len(), 1);
        assert_eq!(by_ids[0].id, id);
    }

    #[test]
    fn test_reset_face_index() {
        let db = TestDb::new("test_reset");
        let id = Uuid::new_v4();
        let media = MediaItem {
            id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: true,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();
        db.repo
            .save_faces(
                id,
                &[Face {
                    id: Uuid::new_v4(),
                    media_id: id,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 1,
                    box_y2: 1,
                    cluster_id: None,
                    person_id: None,
                }],
                &[vec![0.0; 512]],
            )
            .unwrap();

        assert_eq!(db.repo.get_all_face_embeddings().unwrap().len(), 1);

        db.repo.reset_face_index_impl().unwrap();

        assert_eq!(db.repo.get_all_face_embeddings().unwrap().len(), 0);
        let item = db.repo.find_by_id(id).unwrap().unwrap();
        assert_eq!(item.faces_scanned, false);
    }

    #[test]
    fn test_cluster_representatives() {
        let db = TestDb::new("test_reps");
        let id = Uuid::new_v4();
        let media = MediaItem {
            id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: true,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();
        db.repo
            .save_faces(
                id,
                &[Face {
                    id: Uuid::new_v4(),
                    media_id: id,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 1,
                    box_y2: 1,
                    cluster_id: Some(777),
                    person_id: None,
                }],
                &[vec![0.0; 512]],
            )
            .unwrap();

        let reps = db.repo.get_cluster_representatives_impl().unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].0, 777);
        assert_eq!(reps[0].1.id, id);
    }

    #[test]
    fn test_face_vector_retrieval() {
        let db = TestDb::new("test_vec_ret");
        let id = Uuid::new_v4();
        let media = MediaItem {
            id,
            filename: "t.jpg".to_string(),
            original_filename: "t.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "h".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: None,
            height: None,
            size_bytes: 10,
            exif_json: None,
            is_favorite: false,
            faces_scanned: true,
            tags: vec![],
            faces: vec![],
        };
        db.repo.save_metadata_and_vector(&media, None).unwrap();

        let face_id = Uuid::new_v4();
        let vec = vec![0.5; 512];
        db.repo
            .save_faces(
                id,
                &[Face {
                    id: face_id,
                    media_id: id,
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 1,
                    box_y2: 1,
                    cluster_id: None,
                    person_id: None,
                }],
                &[vec.clone()],
            )
            .unwrap();

        let loaded = db.repo.get_face_embedding_impl(face_id).unwrap();
        assert_eq!(loaded.len(), 512);

        let nearest = db.repo.get_nearest_face_embeddings_impl(&vec, 10).unwrap();
        assert_eq!(nearest.len(), 1);
        assert_eq!(nearest[0].0, face_id);
    }
}
