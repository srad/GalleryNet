use super::models::{
    Face, FaceGroup, FaceStats, Folder, MediaCounts, MediaItem, MediaSummary, Person, TagCount,
    TrainedTagModel,
};

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
    #[error("Network error: {0}")]
    Network(String),
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
    fn update_media_and_vector(
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
        person_id: Option<uuid::Uuid>,
        cluster_id: Option<i64>,
        sort_asc: bool,
        sort_by: &str,
    ) -> Result<Vec<MediaSummary>, DomainError>;
    fn media_counts(&self) -> Result<MediaCounts, DomainError>;

    fn set_favorite(&self, id: uuid::Uuid, favorite: bool) -> Result<(), DomainError>;

    fn get_all_tags(&self) -> Result<Vec<TagCount>, DomainError>;
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
        person_id: Option<uuid::Uuid>,
        cluster_id: Option<i64>,
        sort_asc: bool,
        sort_by: &str,
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
    fn get_tag_model(&self, tag_id: i64) -> Result<Option<TrainedTagModel>, DomainError>;
    fn save_tag_model(
        &self,
        tag_id: i64,
        weights: &[f64],
        bias: f64,
        platt_a: f64,
        platt_b: f64,
        trained_at_count: usize,
    ) -> Result<(), DomainError>;
    fn get_last_trained_count(&self, tag_id: i64) -> Result<usize, DomainError>;
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
    fn get_nearest_embeddings(
        &self,
        vector: &[f32],
        limit: usize,
        exclude_ids: &[uuid::Uuid],
    ) -> Result<Vec<(uuid::Uuid, Vec<f32>)>, DomainError>;
    fn get_tag_id_by_name(&self, name: &str) -> Result<Option<i64>, DomainError>;
    fn get_tag_name_by_id(&self, tag_id: i64) -> Result<Option<String>, DomainError>;
    fn get_manual_positives(&self, tag_id: i64) -> Result<Vec<uuid::Uuid>, DomainError>;
    fn get_all_ids_with_tag(&self, tag_id: i64) -> Result<Vec<uuid::Uuid>, DomainError>;
    fn find_media_without_phash(&self) -> Result<Vec<MediaItem>, DomainError>;

    // --- Face operations ---
    fn save_faces(
        &self,
        media_id: uuid::Uuid,
        faces: &[Face],
        embeddings: &[Vec<f32>],
    ) -> Result<(), DomainError>;
    fn get_all_face_embeddings(
        &self,
    ) -> Result<Vec<(uuid::Uuid, uuid::Uuid, Vec<f32>)>, DomainError>;
    fn get_face_embedding(&self, id: uuid::Uuid) -> Result<Vec<f32>, DomainError>;
    fn get_nearest_face_embeddings(
        &self,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<(uuid::Uuid, uuid::Uuid, f32)>, DomainError>;
    fn update_face_clusters(
        &self,
        face_ids_with_clusters: &[(uuid::Uuid, i64)],
    ) -> Result<(), DomainError>;
    fn get_face_groups(&self) -> Result<Vec<FaceGroup>, DomainError>;
    fn get_cluster_representatives(&self) -> Result<Vec<(i64, MediaItem, Face)>, DomainError>;
    fn find_media_unscanned_faces(&self, limit: usize) -> Result<Vec<MediaItem>, DomainError>;
    fn set_faces_scanned(&self, media_id: uuid::Uuid, scanned: bool) -> Result<(), DomainError>;
    fn save_face_indexing_results(
        &self,
        media_id: uuid::Uuid,
        faces: &[Face],
        embeddings: &[Vec<f32>],
    ) -> Result<(), DomainError>;
    fn find_media_missing_embeddings(&self) -> Result<Vec<MediaItem>, DomainError>;
    fn get_media_items_by_ids(&self, ids: &[uuid::Uuid]) -> Result<Vec<MediaItem>, DomainError>;
    fn reset_face_index(&self) -> Result<(), DomainError>;
    fn list_people(
        &self,
        include_hidden: bool,
    ) -> Result<Vec<(Person, Option<Face>, Option<MediaSummary>)>, DomainError>;
    fn get_person(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<(Person, Option<Face>, Option<MediaSummary>)>, DomainError>;
    fn create_person(&self, id: uuid::Uuid, name: &str) -> Result<Person, DomainError>;
    fn update_person(&self, person: &Person) -> Result<(), DomainError>;
    fn delete_person(&self, id: uuid::Uuid) -> Result<(), DomainError>;
    fn rename_person(&self, id: uuid::Uuid, name: &str) -> Result<(), DomainError>;
    fn name_face(&self, face_id: uuid::Uuid, person_id: uuid::Uuid) -> Result<(), DomainError>;
    fn name_cluster(&self, cluster_id: i64, person_id: uuid::Uuid) -> Result<(), DomainError>;
    fn merge_people(&self, source_id: uuid::Uuid, target_id: uuid::Uuid)
        -> Result<(), DomainError>;
    fn assign_people_to_clusters(&self) -> Result<usize, DomainError>;
    fn get_person_photos(&self, person_id: uuid::Uuid) -> Result<Vec<MediaSummary>, DomainError>;
    fn get_face_stats(&self) -> Result<FaceStats, DomainError>;

    fn get_unscanned_media_ids(&self, limit: usize) -> Result<Vec<uuid::Uuid>, DomainError>;
}

pub struct DetectedFace {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub embedding: Vec<f32>,
}

pub trait AiProcessor: Send + Sync {
    fn extract_features(&self, image_bytes: &[u8]) -> Result<Vec<f32>, DomainError>;
    fn detect_and_extract_faces(
        &self,
        image_bytes: &[u8],
    ) -> Result<Vec<DetectedFace>, DomainError>;
}

pub trait HashGenerator: Send + Sync {
    fn generate_phash(&self, image_bytes: &[u8]) -> Result<String, DomainError>;
}
