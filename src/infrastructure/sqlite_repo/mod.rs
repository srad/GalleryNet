mod embeddings;
mod folders;
mod media;
mod tags;

use crate::domain::DomainError;
use rusqlite::{params, Connection};
use std::sync::{Condvar, Mutex};

const POOL_SIZE: usize = 4;

pub struct SqliteRepository {
    pool: Mutex<Vec<Connection>>,
    available: Condvar,
}

impl SqliteRepository {
    pub fn new(path: &str) -> Result<Self, DomainError> {
        println!("Loading sqlite-vec extension...");
        // Load sqlite-vec extension
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        // Create first connection and initialize schema
        println!("Opening initial connection to {}...", path);
        let conn = Self::open_conn(path)?;

        println!("Ensuring media table exists...");
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
        .map_err(|e| DomainError::Database(format!("Failed to create media table: {}", e)))?;

        println!("Ensuring vec_media virtual table exists...");
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_media USING vec0(
                embedding float[1280] distance_metric=cosine
            )",
            [],
        )
        .map_err(|e| DomainError::Database(format!("Failed to create vec_media table: {}", e)))?;

        println!("Ensuring folders table exists...");
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
        .map_err(|e| DomainError::Database(format!("Failed to create folders table: {}", e)))?;

        println!("Ensuring folder_media table exists...");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS folder_media (
                folder_id BLOB NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
                media_id BLOB NOT NULL REFERENCES media(id) ON DELETE CASCADE,
                added_at TEXT NOT NULL,
                PRIMARY KEY (folder_id, media_id)
            )",
            [],
        )
        .map_err(|e| {
            DomainError::Database(format!("Failed to create folder_media table: {}", e))
        })?;

        println!("Ensuring favorites table exists...");
        // Favorites
        conn.execute(
            "CREATE TABLE IF NOT EXISTS favorites (
                media_id BLOB PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| DomainError::Database(format!("Failed to create favorites table: {}", e)))?;

        println!("Ensuring tags table exists...");
        // Tags
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            )",
            [],
        )
        .map_err(|e| DomainError::Database(format!("Failed to create tags table: {}", e)))?;

        println!("Ensuring media_tags table exists...");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS media_tags (
                media_id BLOB NOT NULL REFERENCES media(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                is_auto BOOLEAN NOT NULL DEFAULT 0,
                confidence REAL,
                PRIMARY KEY (media_id, tag_id)
            )",
            [],
        )
        .map_err(|e| DomainError::Database(format!("Failed to create media_tags table: {}", e)))?;

        println!("Checking for migrations...");
        // Migration for existing databases
        let has_is_auto: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('media_tags') WHERE name='is_auto'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if has_is_auto == 0 {
            println!("Adding is_auto column to media_tags...");
            let _ = conn.execute(
                "ALTER TABLE media_tags ADD COLUMN is_auto BOOLEAN NOT NULL DEFAULT 0",
                [],
            );
        }

        let has_confidence: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('media_tags') WHERE name='confidence'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if has_confidence == 0 {
            println!("Adding confidence column to media_tags...");
            let _ = conn.execute("ALTER TABLE media_tags ADD COLUMN confidence REAL", []);
        }

        println!("Ensuring tag_models table exists...");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tag_models (
                tag_id INTEGER PRIMARY KEY REFERENCES tags(id) ON DELETE CASCADE,
                weights BLOB NOT NULL,
                bias REAL NOT NULL,
                platt_a REAL NOT NULL DEFAULT -2.0,
                platt_b REAL NOT NULL DEFAULT 0.0,
                trained_at_count INTEGER NOT NULL DEFAULT 0,
                version INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )
        .map_err(|e| DomainError::Database(format!("Failed to create tag_models table: {}", e)))?;

        // Migration for trained_at_count
        let has_trained_at_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('tag_models') WHERE name='trained_at_count'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if has_trained_at_count == 0 {
            println!("Adding trained_at_count column to tag_models...");
            let _ = conn.execute(
                "ALTER TABLE tag_models ADD COLUMN trained_at_count INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        // Migration for Platt scaling coefficients
        let has_platt_a: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('tag_models') WHERE name='platt_a'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if has_platt_a == 0 {
            println!("Adding Platt scaling columns to tag_models...");
            let _ = conn.execute(
                "ALTER TABLE tag_models ADD COLUMN platt_a REAL NOT NULL DEFAULT -2.0",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE tag_models ADD COLUMN platt_b REAL NOT NULL DEFAULT 0.0",
                [],
            );
        }

        println!("Ensuring idx_media_tags_tag_id index exists...");
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_media_tags_tag_id ON media_tags(tag_id)",
            [],
        )
        .map_err(|e| DomainError::Database(format!("Failed to create index: {}", e)))?;

        println!("Opening connection pool...");
        let mut connections = vec![conn];
        for _ in 1..POOL_SIZE {
            connections.push(Self::open_conn(path)?);
        }

        println!("Database initialization complete.");
        Ok(Self {
            pool: Mutex::new(connections),
            available: Condvar::new(),
        })
    }

    fn open_conn(path: &str) -> Result<Connection, DomainError> {
        let conn = Connection::open(path)
            .map_err(|e| DomainError::Database(format!("Failed to open connection: {}", e)))?;

        // Use standard journaling for maximum compatibility with Docker bind mounts on Windows
        // Use query_row for PRAGMAs that return values to avoid "Execute returned results" warnings
        let _: String = conn
            .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
            .unwrap_or_else(|_| "DELETE".to_string());

        let _: i64 = conn
            .query_row("PRAGMA busy_timeout=10000", [], |r| r.get(0))
            .unwrap_or(10000);

        Ok(conn)
    }

    pub(crate) fn with_conn<T, F>(&self, f: F) -> Result<T, DomainError>
    where
        F: FnOnce(&mut Connection) -> Result<T, DomainError>,
    {
        let mut conn = {
            let mut pool = self.pool.lock().unwrap();
            loop {
                if let Some(conn) = pool.pop() {
                    break conn;
                }
                pool = self.available.wait(pool).unwrap();
            }
        };

        let result = f(&mut conn);

        self.pool.lock().unwrap().push(conn);
        self.available.notify_one();

        result
    }
}

// ---- MediaRepository trait implementation (delegates to submodule _impl methods) ----

use crate::domain::{
    Folder, MediaCounts, MediaItem, MediaRepository, MediaSummary, TagCount, TagDetail,
};

impl MediaRepository for SqliteRepository {
    fn save_metadata_and_vector(
        &self,
        media: &MediaItem,
        vector: Option<&[f32]>,
    ) -> Result<(), DomainError> {
        self.save_metadata_and_vector_impl(media, vector)
    }

    fn exists_by_phash(&self, phash: &str) -> Result<bool, DomainError> {
        self.exists_by_phash_impl(phash)
    }

    fn find_similar(
        &self,
        vector: &[f32],
        limit: usize,
        max_distance: f32,
    ) -> Result<Vec<MediaItem>, DomainError> {
        self.find_similar_impl(vector, limit, max_distance)
    }

    fn find_by_id(&self, id: uuid::Uuid) -> Result<Option<MediaItem>, DomainError> {
        self.find_by_id_impl(id)
    }

    fn delete(&self, id: uuid::Uuid) -> Result<(), DomainError> {
        self.delete_impl(id)
    }

    fn get_embedding(&self, id: uuid::Uuid) -> Result<Option<Vec<f32>>, DomainError> {
        self.get_embedding_impl(id)
    }

    fn delete_many(&self, ids: &[uuid::Uuid]) -> Result<usize, DomainError> {
        self.delete_many_impl(ids)
    }

    fn find_all(
        &self,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        tags: Option<Vec<String>>,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.find_all_impl(limit, offset, media_type, favorite, tags, sort_asc)
    }

    fn media_counts(&self) -> Result<MediaCounts, DomainError> {
        self.media_counts_impl()
    }

    fn set_favorite(&self, id: uuid::Uuid, favorite: bool) -> Result<(), DomainError> {
        self.set_favorite_impl(id, favorite)
    }

    fn get_all_tags(&self) -> Result<Vec<TagCount>, DomainError> {
        self.get_all_tags_impl()
    }

    fn update_media_tags(&self, id: uuid::Uuid, tags: Vec<String>) -> Result<(), DomainError> {
        self.update_media_tags_impl(id, tags)
    }

    fn update_media_tags_batch(
        &self,
        ids: &[uuid::Uuid],
        tags: &[String],
    ) -> Result<(), DomainError> {
        self.update_media_tags_batch_impl(ids, tags)
    }

    fn create_folder(&self, id: uuid::Uuid, name: &str) -> Result<Folder, DomainError> {
        self.create_folder_impl(id, name)
    }

    fn get_folder(&self, id: uuid::Uuid) -> Result<Option<Folder>, DomainError> {
        self.get_folder_impl(id)
    }

    fn list_folders(&self) -> Result<Vec<Folder>, DomainError> {
        self.list_folders_impl()
    }

    fn delete_folder(&self, id: uuid::Uuid) -> Result<(), DomainError> {
        self.delete_folder_impl(id)
    }

    fn rename_folder(&self, id: uuid::Uuid, name: &str) -> Result<(), DomainError> {
        self.rename_folder_impl(id, name)
    }

    fn reorder_folders(&self, order: &[(uuid::Uuid, i64)]) -> Result<(), DomainError> {
        self.reorder_folders_impl(order)
    }

    fn add_media_to_folder(
        &self,
        folder_id: uuid::Uuid,
        media_ids: &[uuid::Uuid],
    ) -> Result<usize, DomainError> {
        self.add_media_to_folder_impl(folder_id, media_ids)
    }

    fn remove_media_from_folder(
        &self,
        folder_id: uuid::Uuid,
        media_ids: &[uuid::Uuid],
    ) -> Result<usize, DomainError> {
        self.remove_media_from_folder_impl(folder_id, media_ids)
    }

    fn find_all_in_folder(
        &self,
        folder_id: uuid::Uuid,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        tags: Option<Vec<String>>,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.find_all_in_folder_impl(
            folder_id, limit, offset, media_type, favorite, tags, sort_asc,
        )
    }

    fn get_folder_media_files(
        &self,
        folder_id: uuid::Uuid,
    ) -> Result<Vec<MediaSummary>, DomainError> {
        self.get_folder_media_files_impl(folder_id)
    }

    fn get_all_embeddings(
        &self,
        folder_id: Option<uuid::Uuid>,
    ) -> Result<Vec<(MediaSummary, Vec<f32>)>, DomainError> {
        self.get_all_embeddings_impl(folder_id)
    }

    // --- Tag Learning ---
    fn get_tag_model(
        &self,
        tag_id: i64,
    ) -> Result<Option<crate::domain::TrainedTagModel>, DomainError> {
        self.get_tag_model_impl(tag_id)
    }

    fn save_tag_model(
        &self,
        tag_id: i64,
        weights: &[f64],
        bias: f64,
        platt_a: f64,
        platt_b: f64,
        trained_at_count: usize,
    ) -> Result<(), DomainError> {
        self.save_tag_model_impl(tag_id, weights, bias, platt_a, platt_b, trained_at_count)
    }

    fn get_last_trained_count(&self, tag_id: i64) -> Result<usize, DomainError> {
        self.get_last_trained_count_impl(tag_id)
    }

    fn get_tags_with_manual_counts(&self) -> Result<Vec<(i64, String, usize)>, DomainError> {
        self.get_tags_with_manual_counts_impl()
    }

    fn get_tags_with_auto_counts(&self) -> Result<Vec<(i64, String, usize)>, DomainError> {
        self.get_tags_with_auto_counts_impl()
    }

    fn count_auto_tags(&self, folder_id: Option<uuid::Uuid>) -> Result<usize, DomainError> {
        self.count_auto_tags_impl(folder_id)
    }

    fn update_auto_tags(
        &self,
        tag_id: i64,
        media_ids_with_scores: &[(uuid::Uuid, f64)],
        scope_media_ids: Option<&[uuid::Uuid]>,
    ) -> Result<(), DomainError> {
        self.update_auto_tags_impl(tag_id, media_ids_with_scores, scope_media_ids)
    }

    fn get_random_embeddings(
        &self,
        limit: usize,
        exclude_ids: &[uuid::Uuid],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, DomainError> {
        self.get_random_embeddings_impl(limit, exclude_ids)
    }

    fn get_nearest_embeddings(
        &self,
        vector: &[f32],
        limit: usize,
        exclude_ids: &[uuid::Uuid],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, DomainError> {
        self.get_nearest_embeddings_impl(vector, limit, exclude_ids)
    }

    fn get_tag_id_by_name(&self, name: &str) -> Result<Option<i64>, DomainError> {
        self.get_tag_id_by_name_impl(name)
    }

    fn get_tag_name_by_id(&self, tag_id: i64) -> Result<Option<String>, DomainError> {
        self.get_tag_name_by_id_impl(tag_id)
    }

    fn get_manual_positives(&self, tag_id: i64) -> Result<Vec<uuid::Uuid>, DomainError> {
        self.get_manual_positives_impl(tag_id)
    }

    fn get_all_ids_with_tag(&self, tag_id: i64) -> Result<Vec<uuid::Uuid>, DomainError> {
        self.get_all_ids_with_tag_impl(tag_id)
    }
}

// ---- Tag helpers shared across submodules ----

/// Load tags for a single media item (by UUID bytes).
pub(crate) fn load_tags_for_media(conn: &Connection, media_id: &[u8]) -> Vec<TagDetail> {
    let mut stmt = match conn.prepare(
        "SELECT t.name, mt.is_auto, mt.confidence 
         FROM tags t 
         JOIN media_tags mt ON mt.tag_id = t.id 
         WHERE mt.media_id = ?1 
         ORDER BY t.name",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map(params![media_id], |row| {
        Ok(TagDetail {
            name: row.get(0)?,
            is_auto: row.get(1)?,
            confidence: row.get(2)?,
        })
    }) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Load tags for multiple media items at once. Returns a map of UUID bytes -> tag list.
pub(crate) fn load_tags_bulk(
    conn: &Connection,
    media_ids: &[Vec<u8>],
) -> std::collections::HashMap<Vec<u8>, Vec<TagDetail>> {
    let mut map: std::collections::HashMap<Vec<u8>, Vec<TagDetail>> =
        std::collections::HashMap::new();
    if media_ids.is_empty() {
        return map;
    }
    for id_bytes in media_ids {
        let tags = load_tags_for_media(conn, id_bytes);
        if !tags.is_empty() {
            map.insert(id_bytes.clone(), tags);
        }
    }
    map
}

pub(crate) fn normalize_vector(vector: &mut [f32]) {
    let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        for v in vector.iter_mut() {
            *v *= inv;
        }
    }
}
