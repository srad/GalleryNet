use crate::domain::{AiProcessor, DomainError, MediaRepository, Face};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

pub struct ScanFacesUseCase {
    repo: Arc<dyn MediaRepository>,
    ai: Arc<dyn AiProcessor>,
    storage_path: PathBuf,
}

impl ScanFacesUseCase {
    pub fn new(
        repo: Arc<dyn MediaRepository>,
        ai: Arc<dyn AiProcessor>,
        storage_path: PathBuf,
    ) -> Self {
        Self {
            repo,
            ai,
            storage_path,
        }
    }

    pub async fn execute(&self) -> Result<usize, DomainError> {
        // 1. Find unscanned media (batch size 10 to avoid blocking too long)
        let candidates = self.repo.find_media_unscanned_faces(10)?;
        if candidates.is_empty() {
            return Ok(0);
        }

        let mut scanned_count = 0;

        for media in candidates {
            // Load file
            let file_path = self.storage_path.join(&media.filename);
            if !file_path.exists() {
                // If file missing, mark scanned to avoid infinite loop
                let _ = self.repo.set_faces_scanned(media.id, true);
                continue;
            }

            let data = match fs::read(&file_path).await {
                Ok(d) => d,
                Err(e) => {
                    println!("Failed to read file {}: {}", media.id, e);
                    continue;
                }
            };

            // Detect faces
            match self.ai.detect_and_extract_faces(&data) {
                Ok(detected) => {
                    let mut faces = Vec::with_capacity(detected.len());
                    let mut embeddings = Vec::with_capacity(detected.len());

                    for f in detected {
                        faces.push(Face {
                            id: Uuid::new_v4(),
                            media_id: media.id,
                            box_x1: f.x1,
                            box_y1: f.y1,
                            box_x2: f.x2,
                            box_y2: f.y2,
                            cluster_id: None,
                            person_id: None,
                        });
                        embeddings.push(f.embedding);
                    }

                    // Save results (this also sets faces_scanned = true)
                    if let Err(e) = self.repo.save_face_indexing_results(media.id, &faces, &embeddings) {
                        println!("Failed to save faces for {}: {}", media.id, e);
                    } else {
                        scanned_count += 1;
                    }
                }
                Err(e) => {
                    println!("Face detection failed for {}: {}", media.id, e);
                    // Mark as scanned anyway so we don't retry forever? 
                    // Or keep it unscanned to retry later? 
                    // For now, let's mark it as scanned if it's a "model error" might retry, 
                    // but if it's "image decode error" maybe skip.
                    // To be safe, let's mark it scanned to progress queue.
                    let _ = self.repo.set_faces_scanned(media.id, true);
                }
            }
        }

        Ok(scanned_count)
    }
}
