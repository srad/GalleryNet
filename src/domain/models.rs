use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDetail {
    pub name: String,
    pub is_auto: bool,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: Uuid,
    pub filename: String,
    pub original_filename: String,
    pub media_type: String,
    pub phash: String,
    pub uploaded_at: DateTime<Utc>,
    pub original_date: DateTime<Utc>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: i64,
    pub exif_json: Option<String>,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub faces_scanned: bool,
    #[serde(default)]
    pub tags: Vec<TagDetail>,
    #[serde(default)]
    pub faces: Vec<Face>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSummary {
    pub id: Uuid,
    pub filename: String,
    pub original_filename: String,
    pub media_type: String,
    pub uploaded_at: DateTime<Utc>,
    pub original_date: DateTime<Utc>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: i64,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub tags: Vec<TagDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaCounts {
    pub total: i64,
    pub images: i64,
    pub videos: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub item_count: i64,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaGroup {
    pub id: usize,
    pub items: Vec<MediaSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Face {
    pub id: Uuid,
    pub media_id: Uuid,
    pub box_x1: i32,
    pub box_y1: i32,
    pub box_x2: i32,
    pub box_y2: i32,
    pub cluster_id: Option<i64>,
    pub person_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    pub is_hidden: bool,
    pub face_count: i64,
    pub representative_face_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceGroup {
    pub id: i64,
    pub items: Vec<MediaSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainedTagModel {
    pub weights: Vec<f64>,
    pub bias: f64,
    pub platt_a: f64,
    pub platt_b: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceStats {
    pub total_faces: i64,
    pub total_people: i64,
    pub named_people: i64,
    pub hidden_people: i64,
    pub unassigned_faces: i64,
    pub ungrouped_faces: i64,
}
