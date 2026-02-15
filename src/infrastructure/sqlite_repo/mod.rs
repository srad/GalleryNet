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

        // Tags
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS media_tags (
                media_id BLOB NOT NULL REFERENCES media(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (media_id, tag_id)
            )",
            [],
        )
        .map_err(|e| DomainError::Database(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_media_tags_tag_id ON media_tags(tag_id)",
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

use crate::domain::{Folder, MediaCounts, MediaItem, MediaRepository, MediaSummary};

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

    fn get_all_tags(&self) -> Result<Vec<String>, DomainError> {
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
}

// ---- Tag helpers shared across submodules ----

/// Load tags for a single media item (by UUID bytes).
pub(crate) fn load_tags_for_media(conn: &Connection, media_id: &[u8]) -> Vec<String> {
    let mut stmt = match conn.prepare(
        "SELECT t.name FROM tags t JOIN media_tags mt ON mt.tag_id = t.id WHERE mt.media_id = ?1 ORDER BY t.name",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map(params![media_id], |row| row.get::<_, String>(0)) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Load tags for multiple media items at once. Returns a map of UUID bytes -> tag list.
pub(crate) fn load_tags_bulk(
    conn: &Connection,
    media_ids: &[Vec<u8>],
) -> std::collections::HashMap<Vec<u8>, Vec<String>> {
    let mut map: std::collections::HashMap<Vec<u8>, Vec<String>> = std::collections::HashMap::new();
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
