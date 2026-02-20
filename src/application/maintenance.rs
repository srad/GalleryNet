use crate::domain::{AiProcessor, DomainError, HashGenerator, MediaRepository, MediaItem};

use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

use super::processor;

pub struct FixThumbnailsUseCase {
    repo: Arc<dyn MediaRepository>,
    ai: Arc<dyn AiProcessor>,
    hasher: Arc<dyn HashGenerator>,
    storage_path: PathBuf,
    thumbnail_path: PathBuf,
}

impl FixThumbnailsUseCase {
    pub fn new(
        repo: Arc<dyn MediaRepository>,
        ai: Arc<dyn AiProcessor>,
        hasher: Arc<dyn HashGenerator>,
        storage_path: PathBuf,
        thumbnail_path: PathBuf,
    ) -> Self {
        Self {
            repo,
            ai,
            hasher,
            storage_path,
            thumbnail_path,
        }
    }

    pub async fn execute(&self) -> Result<Vec<MediaItem>, DomainError> {
        // Find media with 'no_hash' phash
        let candidates = self.repo.find_media_without_phash()?;
        let mut fixed_items = Vec::new();

        for mut media in candidates {
            // Load original file
            let id_str = media.id.to_string();
            let (p1, p2) = (&id_str[0..2], &id_str[2..4]);
            
            // media.filename is like "ab/cd/uuid.mp4"
            // We need to resolve it against storage_path
            // Note: media.filename already contains the sharding structure relative to upload dir
            let file_path = self.storage_path.join(&media.filename);

            if !file_path.exists() {
                println!("Original file missing for {}: {:?}", media.id, file_path);
                continue;
            }

            let data = match fs::read(&file_path).await {
                Ok(d) => d,
                Err(e) => {
                    println!("Failed to read file {}: {}", media.id, e);
                    continue;
                }
            };

            // Process media
            let processed = match processor::process_media(&media.original_filename, &data, self.hasher.as_ref()).await {
                Ok(p) => p,
                Err(e) => {
                    println!("Failed to process media {}: {}", media.id, e);
                    continue;
                }
            };

            // Save thumbnail
            if !processed.thumbnail_bytes.is_empty() {
                let thumb_sub_path = self.thumbnail_path.join(p1).join(p2);
                if let Err(e) = fs::create_dir_all(&thumb_sub_path).await {
                    println!("Failed to create thumbnail dir {}: {}", media.id, e);
                    continue;
                }
                
                let thumb_name = format!("{}.jpg", media.id);
                if let Err(e) = fs::write(thumb_sub_path.join(thumb_name), &processed.thumbnail_bytes).await {
                    println!("Failed to write thumbnail {}: {}", media.id, e);
                    continue;
                }
            }

            // Extract features
            let features = processed.feature_input.as_ref().and_then(|bytes| self.ai.extract_features(bytes).ok());

            // Update Media Item with new data
            media.phash = processed.phash;
            media.width = processed.width;
            media.height = processed.height;
            media.exif_json = processed.exif_json;
            if let Some(date) = processed.original_date {
                media.original_date = date;
            }

            // Save updates

            if let Err(e) = self.repo.update_media_and_vector(&media, features.as_deref()) {
                println!("Failed to update database for {}: {}", media.id, e);
                continue;
            }

            // Detect and save faces
            if let Some(bytes) = processed.feature_input.as_ref() {
                if let Ok(detected) = self.ai.detect_and_extract_faces(bytes) {
                    let mut face_models = Vec::with_capacity(detected.len());
                    let mut face_embeddings = Vec::with_capacity(detected.len());

                    for f in detected {
                        face_models.push(crate::domain::Face {
                            id: Uuid::new_v4(),
                            media_id: media.id,
                            box_x1: f.x1,
                            box_y1: f.y1,
                            box_x2: f.x2,
                            box_y2: f.y2,
                            cluster_id: None,
                        });
                        face_embeddings.push(f.embedding);
                    }

                    if !face_models.is_empty() {
                        let _ = self.repo.save_faces(media.id, &face_models, &face_embeddings);
                    }
                }
            }

            fixed_items.push(media);
        }
        
        Ok(fixed_items)
    }
}

pub struct IndexFacesUseCase {
    repo: Arc<dyn MediaRepository>,
    ai: Arc<dyn AiProcessor>,
    storage_path: PathBuf,
}

impl IndexFacesUseCase {
    pub fn new(
        repo: Arc<dyn MediaRepository>,
        ai: Arc<dyn AiProcessor>,
        storage_path: PathBuf,
    ) -> Self {
        Self { repo, ai, storage_path }
    }

    pub async fn execute(&self, limit: usize) -> Result<usize, DomainError> {
        let candidates = self.repo.find_media_unscanned_faces(limit)?;
        let total_candidates = candidates.len();
        let mut count = 0;

        if total_candidates > 0 {
            tracing::info!("Face indexer: processing {} items", total_candidates);
        } else {
            tracing::info!("Face indexer: no unscanned items found.");
        }

        for media in candidates {
            let file_path = self.storage_path.join(&media.filename);
            if !file_path.exists() {
                let _ = self.repo.mark_faces_scanned(media.id);
                continue;
            }

            let data = match fs::read(&file_path).await {
                Ok(d) => d,
                Err(_) => {
                    let _ = self.repo.mark_faces_scanned(media.id);
                    continue;
                }
            };

            let is_video = matches!(
                std::path::Path::new(&media.filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .as_str(),
                "mp4" | "mov" | "avi" | "mkv" | "webm"
            );

            // Process for faces
            let mut all_detected = Vec::new();
            let mut ai_error = false;

            if is_video {
                if let Ok(frames) = processor::extract_video_frames(&data).await {
                    // Extract faces from each representative frame
                    for frame_bytes in frames {
                        match self.ai.detect_and_extract_faces(&frame_bytes) {
                            Ok(mut detected) => all_detected.append(&mut detected),
                            Err(e) => {
                                tracing::error!("Face detection failed for video frame: {}", e);
                                ai_error = true;
                                break;
                            }
                        }
                    }
                }
            } else {
                match self.ai.detect_and_extract_faces(&data) {
                    Ok(detected) => all_detected = detected,
                    Err(e) => {
                        tracing::error!("Face detection failed: {}", e);
                        ai_error = true;
                    }
                }
            }

            if ai_error {
                // Don't mark as scanned, so we try again next time
                continue;
            }

            if !all_detected.is_empty() {
                let mut face_models = Vec::with_capacity(all_detected.len());
                let mut face_embeddings = Vec::with_capacity(all_detected.len());

                for f in all_detected {
                    face_models.push(crate::domain::Face {
                        id: Uuid::new_v4(),
                        media_id: media.id,
                        box_x1: f.x1,
                        box_y1: f.y1,
                        box_x2: f.x2,
                        box_y2: f.y2,
                        cluster_id: None,
                    });
                    face_embeddings.push(f.embedding);
                }

                self.repo.save_face_indexing_results(media.id, &face_models, &face_embeddings)?;
            } else {
                // Mark as scanned even if no faces found
                self.repo.mark_faces_scanned(media.id)?;
            }
            count += 1;
        }

        Ok(count)
    }
}

