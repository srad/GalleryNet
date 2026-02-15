use crate::domain::{DomainError, Folder, MediaCounts, MediaItem, MediaRepository, MediaSummary};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::sync::{Condvar, Mutex};
use uuid::Uuid;

const POOL_SIZE: usize = 4;

pub struct SqliteRepository {
    pool: Mutex<Vec<Connection>>,
    available: Condvar,
}

impl SqliteRepository {
    pub fn new(path: &str) -> Result<Self, DomainError> {
        // Load sqlite-vec extension
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        // Create first connection and initialize schema
        let conn = Self::open_conn(path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS media (
                id BLOB PRIMARY KEY,
                filename TEXT NOT NULL,
                original_filename TEXT NOT NULL,
                media_type TEXT NOT NULL DEFAULT 'image',
                phash TEXT NOT NULL,
                uploaded_at TEXT NOT NULL,
                original_date TEXT NOT NULL DEFAULT '',
                width INTEGER,
                height INTEGER,
                size_bytes INTEGER NOT NULL,
                exif_json TEXT
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_media USING vec0(
                embedding float[1280] distance_metric=cosine
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        // Virtual folders
        conn.execute(
            "CREATE TABLE IF NOT EXISTS folders (
                id BLOB PRIMARY KEY,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS folder_media (
                folder_id BLOB NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
                media_id BLOB NOT NULL REFERENCES media(id) ON DELETE CASCADE,
                added_at TEXT NOT NULL,
                PRIMARY KEY (folder_id, media_id)
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        // Favorites
        conn.execute(
            "CREATE TABLE IF NOT EXISTS favorites (
                media_id BLOB PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        let mut connections = vec![conn];
        for _ in 1..POOL_SIZE {
            connections.push(Self::open_conn(path)?);
        }

        Ok(Self {
            pool: Mutex::new(connections),
            available: Condvar::new(),
        })
    }

    fn open_conn(path: &str) -> Result<Connection, DomainError> {
        let conn = Connection::open(path).map_err(|e| DomainError::Database(e.to_string()))?;
        // Optimize for performance within 1GB RAM constraint:
        // - WAL mode for concurrency
        // - NORMAL synchronous for faster writes
        // - mmap_size=256MB (safe for 1GB total)
        // - cache_size=-64000 (approx 64MB)
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=10000;
             PRAGMA synchronous=NORMAL;
             PRAGMA mmap_size=268435456;
             PRAGMA cache_size=-64000;",
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;
        Ok(conn)
    }

    fn with_conn<T, F>(&self, f: F) -> Result<T, DomainError>
    where
        F: FnOnce(&Connection) -> Result<T, DomainError>,
    {
        let conn = {
            let mut pool = self.pool.lock().unwrap();
            loop {
                if let Some(conn) = pool.pop() {
                    break conn;
                }
                pool = self.available.wait(pool).unwrap();
            }
        };

        let result = f(&conn);

        self.pool.lock().unwrap().push(conn);
        self.available.notify_one();

        result
    }
}

impl MediaRepository for SqliteRepository {
    fn save_metadata_and_vector(
        &self,
        media: &MediaItem,
        vector: Option<&[f32]>,
    ) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("BEGIN").map_err(|e| DomainError::Database(e.to_string()))?;
            stmt.execute([]).map_err(|e| DomainError::Database(e.to_string()))?;

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

            conn.execute("COMMIT", []).map_err(|e| DomainError::Database(e.to_string()))?;
            Ok(())
        })
    }

    fn exists_by_phash(&self, phash: &str) -> Result<bool, DomainError> {
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

    fn find_similar(
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

            let rows = stmt.query_map(params![vector_bytes, limit as i64, max_distance], |row| {
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

                let id = Uuid::from_slice(&id_bytes).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let uploaded_at = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?
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
                })
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }

            Ok(items)
        })
    }

    fn find_by_id(&self, id: Uuid) -> Result<Option<MediaItem>, DomainError> {
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

                let id = Uuid::from_slice(&id_bytes).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let uploaded_at = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);

                Ok(MediaItem { id, filename, original_filename, media_type, phash, uploaded_at, original_date, width, height, size_bytes, exif_json, is_favorite })
            });

            match result {
                Ok(item) => Ok(Some(item)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    fn delete(&self, id: Uuid) -> Result<(), DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            // Get rowid for vec_media cleanup
            let rowid: Option<i64> = conn
                .prepare("SELECT rowid FROM media WHERE id = ?1")
                .and_then(|mut s| s.query_row(params![id.as_bytes()], |r| r.get(0)))
                .ok();

            // Clean up favorites manually if foreign keys aren't enabled
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

    fn get_embedding(&self, id: Uuid) -> Result<Option<Vec<f32>>, DomainError> {
        self.with_conn(|conn| {
            // 1. Get rowid
            let rowid: Option<i64> = conn
                .prepare("SELECT rowid FROM media WHERE id = ?1")
                .and_then(|mut s| s.query_row(params![id.as_bytes()], |r| r.get(0)))
                .ok();

            let rowid = match rowid {
                Some(r) => r,
                None => return Ok(None),
            };

            // 2. Get embedding
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
                    Ok(Some(floats))
                }
                None => Ok(None),
            }
        })
    }

    fn delete_many(&self, ids: &[Uuid]) -> Result<usize, DomainError> {
        self.with_conn(|conn| {
            conn.execute("BEGIN", [])
                .map_err(|e| DomainError::Database(e.to_string()))?;

            let mut deleted = 0usize;
            for id in ids {
                let rowid: Option<i64> = conn
                    .prepare("SELECT rowid FROM media WHERE id = ?1")
                    .and_then(|mut s| s.query_row(params![id.as_bytes()], |r| r.get(0)))
                    .ok();

                // Clean up favorites manually
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

    fn find_all(
        &self,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let order = if sort_asc { "ASC" } else { "DESC" };
            
            // Base query with join
            let mut sql = "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (f.media_id IS NOT NULL) as is_favorite
                         FROM media m
                         LEFT JOIN favorites f ON f.media_id = m.id".to_string();
            
            let mut conditions = Vec::new();
            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            
            if let Some(mt) = media_type {
                conditions.push("m.media_type = ?");
                params_vec.push(Box::new(mt.to_string()));
            }
            
            if favorite {
                conditions.push("f.media_id IS NOT NULL");
            }
            
            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&conditions.join(" AND "));
            }
            
            sql.push_str(&format!(" ORDER BY m.original_date {} LIMIT ? OFFSET ?", order));
            params_vec.push(Box::new(limit as i64));
            params_vec.push(Box::new(offset as i64));
            
            // Re-map params
            // We have to build params carefully because `?` placeholders are usually 1-indexed in rusqlite if not using names, 
            // but here we are appending strings. Wait, rusqlite uses `?1`, `?2` or just `?`. 
            // If I use `?`, they map sequentially. The original code used `?1`, `?2`.
            // I should stick to `?` and sequential parameters for dynamic query building or carefully index.
            // Using `?` is safer here since I'm pushing to `params_vec` in order.
            
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
                        is_favorite,
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

    fn media_counts(&self) -> Result<MediaCounts, DomainError> {
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

    fn set_favorite(&self, id: Uuid, favorite: bool) -> Result<(), DomainError> {
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

    // --- Folder operations ---

    fn create_folder(&self, id: Uuid, name: &str) -> Result<Folder, DomainError> {
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

    fn get_folder(&self, id: Uuid) -> Result<Option<Folder>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT f.id, f.name, f.created_at, COALESCE(c.cnt, 0), f.sort_order
                 FROM folders f
                 LEFT JOIN (SELECT folder_id, COUNT(*) as cnt FROM folder_media WHERE folder_id = ?1) c
                   ON c.folder_id = f.id
                 WHERE f.id = ?1"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let result = stmt.query_row(params![id.as_bytes()], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let name: String = row.get(1)?;
                let created_at_str: String = row.get(2)?;
                let item_count: i64 = row.get(3)?;
                let sort_order: i64 = row.get(4)?;

                let id = Uuid::from_slice(&id_bytes).map_err(|e|
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);

                Ok(Folder { id, name, created_at, item_count, sort_order })
            });

            match result {
                Ok(folder) => Ok(Some(folder)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DomainError::Database(e.to_string())),
            }
        })
    }

    fn list_folders(&self) -> Result<Vec<Folder>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT f.id, f.name, f.created_at, COALESCE(c.cnt, 0), f.sort_order
                 FROM folders f
                 LEFT JOIN (SELECT folder_id, COUNT(*) as cnt FROM folder_media GROUP BY folder_id) c
                   ON c.folder_id = f.id
                 ORDER BY f.sort_order ASC, f.name COLLATE NOCASE"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map([], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let name: String = row.get(1)?;
                let created_at_str: String = row.get(2)?;
                let item_count: i64 = row.get(3)?;
                let sort_order: i64 = row.get(4)?;

                let id = Uuid::from_slice(&id_bytes).map_err(|e|
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);

                Ok(Folder { id, name, created_at, item_count, sort_order })
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut folders = Vec::new();
            for row in rows {
                folders.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(folders)
        })
    }

    fn delete_folder(&self, id: Uuid) -> Result<(), DomainError> {
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

    fn rename_folder(&self, id: Uuid, name: &str) -> Result<(), DomainError> {
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

    fn reorder_folders(&self, order: &[(Uuid, i64)]) -> Result<(), DomainError> {
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

    fn add_media_to_folder(
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

    fn remove_media_from_folder(
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

    fn find_all_in_folder(
        &self,
        folder_id: Uuid,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let order = if sort_asc { "ASC" } else { "DESC" };
            
            let mut sql = "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, (f.media_id IS NOT NULL) as is_favorite
                           FROM media m
                           JOIN folder_media fm ON fm.media_id = m.id
                           LEFT JOIN favorites f ON f.media_id = m.id
                           WHERE fm.folder_id = ?".to_string();
            
            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
                Box::new(folder_id.as_bytes().to_vec())
            ];
            
            if let Some(mt) = media_type {
                sql.push_str(" AND m.media_type = ?");
                params_vec.push(Box::new(mt.to_string()));
            }

            if favorite {
                sql.push_str(" AND f.media_id IS NOT NULL");
            }
            
            sql.push_str(&format!(" ORDER BY m.original_date {} LIMIT ? OFFSET ?", order));
            params_vec.push(Box::new(limit as i64));
            params_vec.push(Box::new(offset as i64));
            
            let mut stmt = conn.prepare(&sql).map_err(|e| DomainError::Database(e.to_string()))?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let timestamp_str: String = row.get(4)?;
                let original_date_str: String = row.get(5)?;
                let is_favorite: bool = row.get(6)?;

                let id = Uuid::from_slice(&id_bytes).map_err(|e|
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let uploaded_at = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);

                Ok(MediaSummary { id, filename, original_filename, media_type, uploaded_at, original_date, is_favorite })
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(items)
        })
    }

    fn get_folder_media_files(&self, folder_id: Uuid) -> Result<Vec<MediaSummary>, DomainError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date
                 FROM media m
                 JOIN folder_media fm ON fm.media_id = m.id
                 WHERE fm.folder_id = ?1"
            ).map_err(|e| DomainError::Database(e.to_string()))?;

            let rows = stmt.query_map(params![folder_id.as_bytes()], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let timestamp_str: String = row.get(4)?;
                let original_date_str: String = row.get(5)?;

                let id = Uuid::from_slice(&id_bytes).map_err(|e|
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let uploaded_at = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);

                Ok(MediaSummary { id, filename, original_filename, media_type, uploaded_at, original_date, is_favorite: false })
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut items = Vec::new();
            for row in rows {
                items.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(items)
        })
    }

    fn get_all_embeddings(
        &self,
        folder_id: Option<Uuid>,
    ) -> Result<Vec<(MediaSummary, Vec<f32>)>, DomainError> {
        self.with_conn(|conn| {
            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match folder_id {
                Some(fid) => (
                    "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, v.embedding
                     FROM media m
                     JOIN folder_media fm ON fm.media_id = m.id
                     JOIN vec_media v ON v.rowid = m.rowid
                     WHERE fm.folder_id = ?1".to_string(),
                    vec![Box::new(fid.as_bytes().to_vec()) as Box<dyn rusqlite::types::ToSql>],
                ),
                None => (
                    "SELECT m.id, m.filename, m.original_filename, m.media_type, m.uploaded_at, m.original_date, v.embedding
                     FROM media m
                     JOIN vec_media v ON v.rowid = m.rowid".to_string(),
                    vec![],
                ),
            };

            let mut stmt = conn.prepare(&sql).map_err(|e| DomainError::Database(e.to_string()))?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let filename: String = row.get(1)?;
                let original_filename: String = row.get(2)?;
                let media_type: String = row.get(3)?;
                let timestamp_str: String = row.get(4)?;
                let original_date_str: String = row.get(5)?;
                let embedding_bytes: Vec<u8> = row.get(6)?;

                let id = Uuid::from_slice(&id_bytes).map_err(|e|
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
                let uploaded_at = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);
                let original_date = DateTime::parse_from_rfc3339(&original_date_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc);

                let summary = MediaSummary { id, filename, original_filename, media_type, uploaded_at, original_date, is_favorite: false };

                // Parse embedding bytes into f32 vec
                if embedding_bytes.len() % 4 != 0 {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        6, rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid embedding length")),
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
            }).map_err(|e| DomainError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(|e| DomainError::Database(e.to_string()))?);
            }
            Ok(results)
        })
    }
}
