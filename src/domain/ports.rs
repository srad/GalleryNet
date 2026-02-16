use super::models::{Folder, MediaCounts, MediaItem, MediaSummary};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("AI processing error: {0}")]
    Ai(String),
    #[error("Hashing error: {0}")]
    Hashing(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Media already exists")]
    DuplicateMedia,
    #[error("Media not found")]
    NotFound,
    #[error("Model loading error: {0}")]
    ModelLoad(String),
}

impl From<rusqlite::Error> for DomainError {
    fn from(err: rusqlite::Error) -> Self {
        DomainError::Database(err.to_string())
    }
}

pub trait MediaRepository: Send + Sync {
    fn save_metadata_and_vector(
        &self,
        media: &MediaItem,
        vector: Option<&[f32]>,
    ) -> Result<(), DomainError>;
    fn exists_by_phash(&self, phash: &str) -> Result<bool, DomainError>;
    fn find_similar(
        &self,
        vector: &[f32],
        limit: usize,
        max_distance: f32,
    ) -> Result<Vec<MediaItem>, DomainError>;
    fn find_by_id(&self, id: uuid::Uuid) -> Result<Option<MediaItem>, DomainError>;
    fn delete(&self, id: uuid::Uuid) -> Result<(), DomainError>;
    fn get_embedding(&self, id: uuid::Uuid) -> Result<Option<Vec<f32>>, DomainError>;
    fn delete_many(&self, ids: &[uuid::Uuid]) -> Result<usize, DomainError>;
    fn find_all(
        &self,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        tags: Option<Vec<String>>,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError>;
    fn media_counts(&self) -> Result<MediaCounts, DomainError>;

    fn set_favorite(&self, id: uuid::Uuid, favorite: bool) -> Result<(), DomainError>;

    fn get_all_tags(&self) -> Result<Vec<String>, DomainError>;
    fn update_media_tags(&self, id: uuid::Uuid, tags: Vec<String>) -> Result<(), DomainError>;
    fn update_media_tags_batch(
        &self,
        ids: &[uuid::Uuid],
        tags: &[String],
    ) -> Result<(), DomainError>;

    // --- Folder operations ---
    fn create_folder(&self, id: uuid::Uuid, name: &str) -> Result<Folder, DomainError>;
    fn get_folder(&self, id: uuid::Uuid) -> Result<Option<Folder>, DomainError>;
    fn list_folders(&self) -> Result<Vec<Folder>, DomainError>;
    fn delete_folder(&self, id: uuid::Uuid) -> Result<(), DomainError>;
    fn rename_folder(&self, id: uuid::Uuid, name: &str) -> Result<(), DomainError>;
    /// Update the sort_order for each folder. The vec contains (folder_id, new_sort_order) pairs.
    fn reorder_folders(&self, order: &[(uuid::Uuid, i64)]) -> Result<(), DomainError>;
    fn add_media_to_folder(
        &self,
        folder_id: uuid::Uuid,
        media_ids: &[uuid::Uuid],
    ) -> Result<usize, DomainError>;
    fn remove_media_from_folder(
        &self,
        folder_id: uuid::Uuid,
        media_ids: &[uuid::Uuid],
    ) -> Result<usize, DomainError>;
    fn find_all_in_folder(
        &self,
        folder_id: uuid::Uuid,
        limit: usize,
        offset: usize,
        media_type: Option<&str>,
        favorite: bool,
        tags: Option<Vec<String>>,
        sort_asc: bool,
    ) -> Result<Vec<MediaSummary>, DomainError>;
    fn get_folder_media_files(
        &self,
        folder_id: uuid::Uuid,
    ) -> Result<Vec<MediaSummary>, DomainError>;
    /// Return all media summaries that have embeddings (scoped to folder if given)
    /// together with their L2-normalized embedding vectors.
    /// Vectors are normalized in-place so that cosine distance = 1 - dot(a, b).
    fn get_all_embeddings(
        &self,
        folder_id: Option<uuid::Uuid>,
    ) -> Result<Vec<(MediaSummary, Vec<f32>)>, DomainError>;

    // --- Tag Learning ---
    fn save_tag_model(&self, tag_id: i64, weights: &[f64], bias: f64) -> Result<(), DomainError>;
    fn get_tags_with_manual_counts(&self) -> Result<Vec<(i64, String, usize)>, DomainError>;
    fn get_tags_with_auto_counts(&self) -> Result<Vec<(i64, String, usize)>, DomainError>;
    fn count_auto_tags(&self, folder_id: Option<uuid::Uuid>) -> Result<usize, DomainError>;
    fn update_auto_tags(
        &self,
        tag_id: i64,
        media_ids_with_scores: &[(uuid::Uuid, f64)],
        scope_media_ids: Option<&[uuid::Uuid]>,
    ) -> Result<(), DomainError>;
    fn get_random_embeddings(
        &self,
        limit: usize,
        exclude_ids: &[uuid::Uuid],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, DomainError>;
    fn get_tag_id_by_name(&self, name: &str) -> Result<Option<i64>, DomainError>;
    fn get_manual_positives(&self, tag_id: i64) -> Result<Vec<uuid::Uuid>, DomainError>;
    fn get_all_ids_with_tag(&self, tag_id: i64) -> Result<Vec<uuid::Uuid>, DomainError>;
}

pub trait AiProcessor: Send + Sync {
    fn extract_features(&self, image_bytes: &[u8]) -> Result<Vec<f32>, DomainError>;
}

pub trait HashGenerator: Send + Sync {
    fn generate_phash(&self, image_bytes: &[u8]) -> Result<String, DomainError>;
}
