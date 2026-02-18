use crate::domain::{DomainError, HashGenerator};
use chrono::{DateTime, NaiveDateTime, Utc};
use exif::Tag;
use image::imageops::FilterType;
use std::io::Cursor;
use std::path::Path;
use tokio::fs;
use uuid::Uuid;

/// Maximum image dimension (width or height) in pixels.
pub const MAX_IMAGE_DIMENSION: u32 = 65_000;

/// Maximum memory allocation for image decoding (~500 MB).
pub const MAX_IMAGE_ALLOC: u64 = 500 * 1024 * 1024;

pub struct ProcessedMedia {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub exif_json: Option<String>,
    pub thumbnail_bytes: Vec<u8>,
    pub phash: String,
    pub feature_input: Option<Vec<u8>>,
    pub original_date: Option<DateTime<Utc>>,
}

/// Load an image with dimension and allocation limits to prevent pixel bombs.
pub fn load_image_with_limits(data: &[u8]) -> Result<image::DynamicImage, DomainError> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| DomainError::Io(format!("Failed to detect image format: {}", e)))?;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC);

    let mut reader = reader;
    reader.limits(limits);

    reader
        .decode()
        .map_err(|e| DomainError::Io(format!("Image decode failed: {}", e)))
}

pub fn apply_orientation(img: image::DynamicImage, orientation: u32) -> image::DynamicImage {
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
pub fn parse_exif_datetime(s: &str) -> Option<DateTime<Utc>> {
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

/// Extract up to 5 representative video frames using ffmpeg's thumbnail filter.
/// The filter picks the most visually distinct frame from each group, avoiding
/// dark/blank frames that are common at video boundaries.
pub async fn extract_video_frames(data: &[u8]) -> Result<Vec<Vec<u8>>, DomainError> {
    let temp_dir = std::env::temp_dir();
    let id = Uuid::new_v4();
    let input_path = temp_dir.join(format!("gallerynet_{}.tmp", id));
    let output_pattern = temp_dir.join(format!("gallerynet_{}_%03d.jpg", id));

    fs::write(&input_path, data)
        .await
        .map_err(|e| DomainError::Io(e.to_string()))?;

    // thumbnail=60 picks the best frame from every 60 frames (~2s at 30fps)
    // scale limits max resolution to 1080p to reduce memory usage during buffering
    let output = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            input_path.to_str().unwrap(),
            "-vf",
            "scale='min(1920,iw)':-2,thumbnail=60",
            "-frames:v",
            "5",
            "-fps_mode",
            "vfr",
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
        return Err(DomainError::Io(
            "ffmpeg failed to extract frames".to_string(),
        ));
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

pub async fn process_media(
    filename: &str,
    data: &[u8],
    hasher: &dyn HashGenerator,
) -> Result<ProcessedMedia, DomainError> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("bin")
        .to_lowercase();

    let is_video = matches!(extension.as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm");

    let mut width = None;
    let mut height = None;
    let mut exif_json = None;
    let mut thumbnail_bytes = Vec::new();
    let mut phash = "no_hash".to_string();
    let mut feature_input: Option<Vec<u8>> = None;
    let mut original_date: Option<DateTime<Utc>> = None;

    if is_video {
        // Extract representative frames via ffmpeg for phash, thumbnail, and features
        if let Ok(frames) = extract_video_frames(data).await {
            // Use the first representative frame for thumbnail and features
            if let Some(first) = frames.first() {
                if let Ok(img) = load_image_with_limits(first) {
                    width = Some(img.width());
                    height = Some(img.height());

                    let thumb = img.resize_to_fill(224, 224, FilterType::CatmullRom);
                    let mut cursor = Cursor::new(&mut thumbnail_bytes);
                    let _ = thumb.write_to(&mut cursor, image::ImageFormat::Jpeg);
                }
                feature_input = Some(first.clone());
            }

            // Combine phashes from all frames for robust duplicate detection
            let frame_hashes: Vec<String> = frames
                .iter()
                .filter_map(|f| hasher.generate_phash(f).ok())
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

        if let Ok(mut img) = load_image_with_limits(data) {
            img = apply_orientation(img, orientation);

            width = Some(img.width());
            height = Some(img.height());

            // Encode oriented image for phash (so rotated duplicates are detected)
            let mut phash_buf = Vec::new();
            img.write_to(&mut Cursor::new(&mut phash_buf), image::ImageFormat::Jpeg)
                .map_err(|e| DomainError::Io(format!("Failed to encode for phash: {}", e)))?;
            phash = hasher
                .generate_phash(&phash_buf)
                .unwrap_or_else(|_| "no_hash".to_string());

            // Thumbnail
            let thumb = img.resize_to_fill(224, 224, FilterType::CatmullRom);
            let mut cursor = Cursor::new(&mut thumbnail_bytes);
            thumb
                .write_to(&mut cursor, image::ImageFormat::Jpeg)
                .map_err(|e| DomainError::Io(format!("Failed to encode thumbnail: {}", e)))?;
        }

        feature_input = Some(data.to_vec());
    }

    Ok(ProcessedMedia {
        width,
        height,
        exif_json,
        thumbnail_bytes,
        phash,
        feature_input,
        original_date,
    })
}
