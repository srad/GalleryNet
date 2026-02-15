use crate::domain::{MediaRepository, AiProcessor, HashGenerator, MediaItem, DomainError};
use std::sync::Arc;
use std::path::PathBuf;
use uuid::Uuid;
use chrono::{Datelike, DateTime, NaiveDateTime, Utc};
use tokio::fs;
use image::imageops::FilterType;
use std::io::Cursor;
use exif::Tag;

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

        let is_video = matches!(extension.as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm");
        let size_bytes = data.len() as i64;

        let mut width = None;
        let mut height = None;
        let mut exif_json = None;
        let mut thumbnail_bytes = Vec::new();
        let mut phash = "no_hash".to_string();
        let mut feature_input: Option<Vec<u8>> = None;
        let mut original_date: Option<DateTime<Utc>> = None;

        if is_video {
            // Extract representative frames via ffmpeg for phash, thumbnail, and features
            if let Ok(frames) = Self::extract_video_frames(data).await {
                // Use the first representative frame for thumbnail and features
                if let Some(first) = frames.first() {
                    if let Ok(img) = image::load_from_memory(first) {
                        width = Some(img.width());
                        height = Some(img.height());

                        let thumb = img.resize_to_fill(224, 224, FilterType::CatmullRom);
                        let mut cursor = Cursor::new(&mut thumbnail_bytes);
                        let _ = thumb.write_to(&mut cursor, image::ImageFormat::Jpeg);
                    }
                    feature_input = Some(first.clone());
                }

                // Combine phashes from all frames for robust duplicate detection
                let frame_hashes: Vec<String> = frames.iter()
                    .filter_map(|f| self.hasher.generate_phash(f).ok())
                    .collect();
                if !frame_hashes.is_empty() {
                    phash = frame_hashes.join("|");
                }
            }
        } else {
            // Parse EXIF
            let mut orientation = 1u32;
            let reader = exif::Reader::new();
            if let Ok(exif) = reader.read_from_container(&mut Cursor::new(data)) {
                let mut map = serde_json::Map::new();
                for f in exif.fields() {
                    let key = f.tag.to_string();
                    let val = f.display_value().with_unit(&exif).to_string();
                    map.insert(key, serde_json::Value::String(val));
                }
                exif_json = serde_json::to_string(&map).ok();

                if let Some(field) = exif.get_field(Tag::Orientation, exif::In::PRIMARY) {
                    if let Some(val) = field.value.get_uint(0) {
                        orientation = val;
                    }
                }

                // Extract original date from EXIF (try DateTimeOriginal, DateTimeDigitized, DateTime)
                for tag in &[Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime] {
                    if let Some(field) = exif.get_field(*tag, exif::In::PRIMARY) {
                        let val_str = field.display_value().to_string();
                        if let Some(dt) = parse_exif_datetime(&val_str) {
                            original_date = Some(dt);
                            break;
                        }
                    }
                }
            }

            if let Ok(mut img) = image::load_from_memory(data) {
                img = apply_orientation(img, orientation);

                width = Some(img.width());
                height = Some(img.height());

                // Encode oriented image for phash (so rotated duplicates are detected)
                let mut phash_buf = Vec::new();
                img.write_to(&mut Cursor::new(&mut phash_buf), image::ImageFormat::Jpeg)
                    .map_err(|e| DomainError::Io(format!("Failed to encode for phash: {}", e)))?;
                phash = self.hasher.generate_phash(&phash_buf).unwrap_or_else(|_| "no_hash".to_string());

                // Thumbnail
                let thumb = img.resize_to_fill(224, 224, FilterType::CatmullRom);
                let mut cursor = Cursor::new(&mut thumbnail_bytes);
                thumb.write_to(&mut cursor, image::ImageFormat::Jpeg)
                    .map_err(|e| DomainError::Io(format!("Failed to encode thumbnail: {}", e)))?;
            }

            feature_input = Some(data.to_vec());
        }

        // Check for duplicates
        if phash != "no_hash" && self.repo.exists_by_phash(&phash)? {
            return Err(DomainError::DuplicateMedia);
        }

        // Extract features
        let features = feature_input.and_then(|bytes| self.ai.extract_features(&bytes).ok());

        // Save to disk
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let (p1, p2) = (&id_str[0..2], &id_str[2..4]);

        let sub_path = self.storage_path.join(p1).join(p2);
        fs::create_dir_all(&sub_path).await
            .map_err(|e| DomainError::Io(e.to_string()))?;

        let thumb_sub_path = self.thumbnail_path.join(p1).join(p2);
        fs::create_dir_all(&thumb_sub_path).await
            .map_err(|e| DomainError::Io(e.to_string()))?;

        let file_name = format!("{}.{}", id, extension);
        let file_path = sub_path.join(&file_name);

        fs::write(&file_path, data).await
            .map_err(|e| DomainError::Io(e.to_string()))?;

        if !thumbnail_bytes.is_empty() {
            let thumb_name = format!("{}.jpg", id);
            fs::write(thumb_sub_path.join(thumb_name), &thumbnail_bytes).await
                .map_err(|e| DomainError::Io(e.to_string()))?;
        }

        let saved_filename = format!("{}/{}/{}", p1, p2, file_name);

        let media_type = if is_video { "video" } else { "image" }.to_string();

        let now = Utc::now();

        // Resolve original_date: EXIF -> filename pattern -> upload time
        let original_date = original_date
            .or_else(|| parse_date_from_filename(&filename))
            .unwrap_or(now);

        let media = MediaItem {
            id,
            filename: saved_filename,
            original_filename: filename,
            media_type,
            phash,
            uploaded_at: now,
            original_date,
            width,
            height,
            size_bytes,
            exif_json,
        };

        self.repo.save_metadata_and_vector(&media, features.as_deref())?;

        Ok(media)
    }

    /// Extract up to 5 representative video frames using ffmpeg's thumbnail filter.
    /// The filter picks the most visually distinct frame from each group, avoiding
    /// dark/blank frames that are common at video boundaries.
    async fn extract_video_frames(data: &[u8]) -> Result<Vec<Vec<u8>>, DomainError> {
        let temp_dir = std::env::temp_dir();
        let id = Uuid::new_v4();
        let input_path = temp_dir.join(format!("gallerynet_{}.tmp", id));
        let output_pattern = temp_dir.join(format!("gallerynet_{}_%03d.jpg", id));

        fs::write(&input_path, data).await
            .map_err(|e| DomainError::Io(e.to_string()))?;

        // thumbnail=300 picks the best frame from every 300 frames (~10s at 30fps)
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y", "-i", input_path.to_str().unwrap(),
                "-vf", "thumbnail=300",
                "-frames:v", "5",
                "-vsync", "vfr",
                output_pattern.to_str().unwrap(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| DomainError::Io(format!("ffmpeg not available: {}", e)))?;

        let _ = fs::remove_file(&input_path).await;

        if !output.status.success() {
            // Clean up any partial output
            for i in 1..=5 {
                let p = temp_dir.join(format!("gallerynet_{}_{:03}.jpg", id, i));
                let _ = fs::remove_file(&p).await;
            }
            return Err(DomainError::Io("ffmpeg failed to extract frames".to_string()));
        }

        // Read all extracted frames
        let mut frames = Vec::new();
        for i in 1..=5 {
            let frame_path = temp_dir.join(format!("gallerynet_{}_{:03}.jpg", id, i));
            if let Ok(bytes) = fs::read(&frame_path).await {
                frames.push(bytes);
            }
            let _ = fs::remove_file(&frame_path).await;
        }

        if frames.is_empty() {
            return Err(DomainError::Io("ffmpeg produced no frames".to_string()));
        }

        Ok(frames)
    }
}

fn apply_orientation(img: image::DynamicImage, orientation: u32) -> image::DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// Parse an EXIF datetime string like "2024-01-15 13:45:30" or "2024:01:15 13:45:30"
/// into a UTC DateTime.
fn parse_exif_datetime(s: &str) -> Option<DateTime<Utc>> {
    // EXIF uses "YYYY:MM:DD HH:MM:SS" format, but display_value may vary
    let normalized = s.trim().replace('/', "-");

    // Try "YYYY:MM:DD HH:MM:SS" (standard EXIF)
    if let Ok(dt) = NaiveDateTime::parse_from_str(&normalized, "%Y:%m:%d %H:%M:%S") {
        return Some(dt.and_utc());
    }
    // Try "YYYY-MM-DD HH:MM:SS" (common alternative)
    if let Ok(dt) = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc());
    }
    // Try "YYYY:MM:DD" (date only)
    if let Ok(dt) = chrono::NaiveDate::parse_from_str(&normalized, "%Y:%m:%d") {
        return Some(dt.and_hms_opt(0, 0, 0)?.and_utc());
    }
    // Try "YYYY-MM-DD" (date only)
    if let Ok(dt) = chrono::NaiveDate::parse_from_str(&normalized, "%Y-%m-%d") {
        return Some(dt.and_hms_opt(0, 0, 0)?.and_utc());
    }
    None
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
