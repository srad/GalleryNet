use crate::domain::{MediaRepository, AiProcessor, MediaItem, DomainError};
use std::sync::Arc;
use uuid::Uuid;

pub struct SearchSimilarUseCase {
    repo: Arc<dyn MediaRepository>,
    ai: Arc<dyn AiProcessor>,
    storage_path: std::path::PathBuf,
}

impl SearchSimilarUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>, ai: Arc<dyn AiProcessor>, storage_path: std::path::PathBuf) -> Self {
        Self { repo, ai, storage_path }
    }

    pub async fn execute(&self, image_bytes: &[u8], limit: usize, max_distance: f32) -> Result<Vec<MediaItem>, DomainError> {
        // 1. Extract features
        let vector = self.ai.extract_features(image_bytes)?;

        // 2. Find similar
        let results = self.repo.find_similar(&vector, limit, max_distance)?;

        Ok(results)
    }

    pub async fn execute_by_id(&self, id: Uuid, limit: usize, max_distance: f32) -> Result<Vec<MediaItem>, DomainError> {
        // 1. Get embedding for the existing item
        let vector = self.repo.get_embedding(id)?
            .ok_or(DomainError::NotFound)?;

        // 2. Find similar (fetch limit + 1 to account for the item itself)
        let results = self.repo.find_similar(&vector, limit + 1, max_distance)?;

        // 3. Filter out the source item itself (distance 0)
        let filtered: Vec<MediaItem> = results.into_iter()
            .filter(|item| item.id != id)
            .take(limit)
            .collect();

        Ok(filtered)
    }

    pub async fn reindex_item(&self, item: &MediaItem) -> Result<(), DomainError> {
        let file_path = self.storage_path.join(&item.filename);
        if !file_path.exists() {
            return Ok(());
        }

        let data = std::fs::read(&file_path)
            .map_err(|e| DomainError::Io(e.to_string()))?;

        // 1. Extract features
        let vector = self.ai.extract_features(&data)?;

        // 2. Update media with vector (this will use update_media_and_vector_impl)
        self.repo.update_media_and_vector(item, Some(&vector))?;

        Ok(())
    }
}

