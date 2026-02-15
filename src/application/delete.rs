use crate::domain::{MediaRepository, DomainError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

pub struct DeleteMediaUseCase {
    repo: Arc<dyn MediaRepository>,
    storage_path: PathBuf,
    thumbnail_path: PathBuf,
}

impl DeleteMediaUseCase {
    pub fn new(
        repo: Arc<dyn MediaRepository>,
        storage_path: PathBuf,
        thumbnail_path: PathBuf,
    ) -> Self {
        Self { repo, storage_path, thumbnail_path }
    }

    pub async fn execute(&self, id: Uuid) -> Result<(), DomainError> {
        let media = self.repo.find_by_id(id)?
            .ok_or(DomainError::NotFound)?;

        self.repo.delete(id)?;
        self.delete_files(&media.filename, id).await;

        Ok(())
    }

    pub async fn execute_batch(&self, ids: &[Uuid]) -> Result<usize, DomainError> {
        // Look up filenames before deleting from DB
        let items: Vec<_> = ids.iter()
            .filter_map(|id| self.repo.find_by_id(*id).ok().flatten())
            .collect();

        let deleted = self.repo.delete_many(ids)?;

        // Clean up files for all found items
        for item in &items {
            self.delete_files(&item.filename, item.id).await;
        }

        Ok(deleted)
    }

    async fn delete_files(&self, filename: &str, id: Uuid) {
        let _ = fs::remove_file(self.storage_path.join(filename)).await;

        let id_str = id.to_string();
        let (p1, p2) = (&id_str[0..2], &id_str[2..4]);
        let thumb_path = self.thumbnail_path.join(p1).join(p2).join(format!("{}.jpg", id));
        let _ = fs::remove_file(&thumb_path).await;
    }
}
