use crate::domain::{MediaRepository, MediaSummary, DomainError};
use std::sync::Arc;

pub struct ListMediaUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl ListMediaUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, page: usize, page_size: usize, media_type: Option<&str>, favorite: bool, tags: Option<Vec<String>>, sort_asc: bool, sort_by: &str) -> Result<Vec<MediaSummary>, DomainError> {
        let limit = page_size;
        let offset = (page - 1) * page_size;

        self.repo.find_all(limit, offset, media_type, favorite, tags, sort_asc, sort_by)
    }
}
