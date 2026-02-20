use crate::domain::{MediaRepository, AiProcessor, HashGenerator, MediaItem, DomainError};
use std::sync::Arc;
use std::path::PathBuf;
use uuid::Uuid;
use chrono::{Datelike, DateTime, NaiveDateTime, Utc};
use tokio::fs;

use super::processor;

/// Allowed file extensions for upload (images + videos).
const ALLOWED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "heic", "heif", "avif",
    "mp4", "mov", "avi", "mkv", "webm",
];

pub struct UploadMediaUseCase {
    repo: Arc<dyn MediaRepository>,
    ai: Arc<dyn AiProcessor>,
    hasher: Arc<dyn HashGenerator>,
    storage_path: PathBuf,
    thumbnail_path: PathBuf,
}

impl UploadMediaUseCase {
    pub fn new(
        repo: Arc<dyn MediaRepository>,
        ai: Arc<dyn AiProcessor>,
        hasher: Arc<dyn HashGenerator>,
        storage_path: PathBuf,
        thumbnail_path: PathBuf,
    ) -> Self {
        Self { repo, ai, hasher, storage_path, thumbnail_path }
    }

    pub async fn execute(&self, filename: String, data: &[u8]) -> Result<MediaItem, DomainError> {
        let extension = std::path::Path::new(&filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("bin")
            .to_lowercase();

        // Reject files with unlisted extensions
        if !ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
            return Err(DomainError::Io(format!("File type not allowed: .{}", extension)));
        }

        let is_video = matches!(extension.as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm");
        let size_bytes = data.len() as i64;

        // Process media using the extracted processor logic
        let processed = processor::process_media(&filename, data, self.hasher.as_ref()).await?;

        // Check for duplicates
        if processed.phash != "no_hash" && self.repo.exists_by_phash(&processed.phash)? {
            return Err(DomainError::DuplicateMedia);
        }

        // Extract features
        let features = processed.feature_input.as_ref().and_then(|bytes| self.ai.extract_features(bytes).ok());

        // Save to disk
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let (p1, p2) = (&id_str[0..2], &id_str[2..4]);

        let sub_path = self.storage_path.join(p1).join(p2);
        fs::create_dir_all(&sub_path).await
            .map_err(|e: std::io::Error| DomainError::Io(e.to_string()))?;

        let thumb_sub_path = self.thumbnail_path.join(p1).join(p2);
        fs::create_dir_all(&thumb_sub_path).await
            .map_err(|e: std::io::Error| DomainError::Io(e.to_string()))?;

        let file_name = format!("{}.{}", id, extension);
        let file_path = sub_path.join(&file_name);

        fs::write(&file_path, data).await
            .map_err(|e: std::io::Error| DomainError::Io(e.to_string()))?;

        if !processed.thumbnail_bytes.is_empty() {
            let thumb_name = format!("{}.jpg", id);
            fs::write(thumb_sub_path.join(thumb_name), &processed.thumbnail_bytes).await
                .map_err(|e: std::io::Error| DomainError::Io(e.to_string()))?;
        }

        let saved_filename = format!("{}/{}/{}", p1, p2, file_name);

        let media_type = if is_video { "video" } else { "image" }.to_string();

        let now = Utc::now();

        // Resolve original_date: EXIF -> filename pattern -> upload time
        let original_date = processed.original_date
            .or_else(|| parse_date_from_filename(&filename))
            .unwrap_or(now);

        let media = MediaItem {
            id,
            filename: saved_filename,
            original_filename: filename,
            media_type,
            phash: processed.phash,
            uploaded_at: now,
            original_date,
            width: processed.width,
            height: processed.height,
            size_bytes,
            exif_json: processed.exif_json,
            is_favorite: false,
            tags: vec![],
            faces: Vec::new(),
            faces_scanned: false, // Don't mark as scanned yet
        };


        self.repo.save_metadata_and_vector(&media, features.as_deref())?;

        // Extract and save faces immediately
        if let Some(bytes) = processed.feature_input.as_ref() {
            if let Ok(detected) = self.ai.detect_and_extract_faces(bytes) {
                let mut face_models = Vec::with_capacity(detected.len());
                let mut face_embeddings = Vec::with_capacity(detected.len());

                for f in detected {
                    face_models.push(crate::domain::Face {
                        id: Uuid::new_v4(),
                        media_id: id,
                        box_x1: f.x1,
                        box_y1: f.y1,
                        box_x2: f.x2,
                        box_y2: f.y2,
                        cluster_id: None,
                    });
                    face_embeddings.push(f.embedding);
                }

                // Use the atomic save results method which also marks as scanned
                self.repo.save_face_indexing_results(id, &face_models, &face_embeddings)?;
            } else {
                // If AI failed during upload, leave faces_scanned=false
                // the background task will try again later.
            }
        } else {
            // No feature input? (e.g. video frames extraction failed)
            // Just mark as scanned so we don't keep trying.
            self.repo.mark_faces_scanned(id)?;
        }


        Ok(media)
    }
}

/// Try to extract a date from the filename using common patterns:
///   - IMG_20240115_134530.jpg
///   - 20240115_134530.jpg
///   - 2024-01-15_13-45-30.jpg
///   - VID_20240115_134530.mp4
///   - Screenshot_20240115-134530.png
///   - PXL_20240115_134530123.jpg (Pixel phones, extra ms digits)
fn parse_date_from_filename(filename: &str) -> Option<DateTime<Utc>> {
    // Strip path components to get just the filename
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    // Strip extension
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);

    // Look for 8-digit date (YYYYMMDD), optionally followed by separator + 6-digit time (HHMMSS)
    // This covers: IMG_20240115_134530, 20240115_134530, VID_20240115_134530,
    //              Screenshot_20240115-134530, PXL_20240115_134530123
    let digits: String = stem.chars().filter(|c| c.is_ascii_digit()).collect();

    // Need at least 8 digits for a date
    if digits.len() >= 14 {
        // Try YYYYMMDDHHMMSS
        if let Ok(dt) = NaiveDateTime::parse_from_str(&digits[..14], "%Y%m%d%H%M%S") {
            if dt.and_utc().year() >= 1970 && dt.and_utc().year() <= 2100 {
                return Some(dt.and_utc());
            }
        }
    }
    if digits.len() >= 8 {
        // Try YYYYMMDD only
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&digits[..8], "%Y%m%d") {
            if d.and_hms_opt(0, 0, 0)?.and_utc().year() >= 1970
                && d.and_hms_opt(0, 0, 0)?.and_utc().year() <= 2100
            {
                return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
            }
        }
    }

    // Try "YYYY-MM-DD" pattern with separators (e.g., 2024-01-15_13-45-30)
    // Already handled by the digit extraction above in most cases

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_extensions_accepted() {
        for ext in ALLOWED_EXTENSIONS {
            assert!(
                ALLOWED_EXTENSIONS.contains(ext),
                "Extension {} should be allowed",
                ext
            );
        }
    }

    #[test]
    fn dangerous_extensions_rejected() {
        let dangerous = ["html", "htm", "svg", "exe", "js", "php", "sh", "bat", "cmd"];
        for ext in &dangerous {
            assert!(
                !ALLOWED_EXTENSIONS.contains(ext),
                "Extension {} must not be in allowlist",
                ext
            );
        }
    }
}
