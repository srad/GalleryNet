use axum::{
    body::Body,
    extract::{ConnectInfo, Multipart, State, Query, Path},
    http::{StatusCode, header, HeaderMap},
    response::{Json, IntoResponse},
    routing::{get, post, put},
    Router,
};
use async_zip::tokio::write::ZipFileWriter;
use async_zip::{ZipEntryBuilder, Compression};
use tokio_util::compat::{TokioAsyncWriteCompatExt, FuturesAsyncWriteCompatExt};

use serde_json::json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore};
use std::path::PathBuf;
use tokio_util::io::ReaderStream;
use tracing::{error, info};

use uuid::Uuid;
use tokio::io::AsyncWriteExt;

use crate::application::{UploadMediaUseCase, SearchSimilarUseCase, ListMediaUseCase, DeleteMediaUseCase, GroupMediaUseCase, TagLearningUseCase};
use crate::domain::{DomainError, MediaItem, MediaRepository};
use crate::presentation::auth::AuthConfig;

/// Maximum page limit for list endpoints.
const MAX_PAGE_LIMIT: usize = 200;

/// Maximum number of files in a single upload request.
const MAX_UPLOAD_FILES: usize = 1_000;

/// Maximum uncompressed size per zip archive part (~2 GB). When the total
/// exceeds this, files are split into roughly equal-sized archives.
const MAX_ZIP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Maximum login attempts per IP within the rate limit window.
const MAX_LOGIN_ATTEMPTS: u32 = 10;

/// Rate limit window duration in seconds.
const RATE_LIMIT_WINDOW_SECS: u64 = 300; // 5 minutes


#[derive(Clone, Serialize)]
pub struct DownloadPart {
    pub id: String,
    pub filename: String,
    pub size_estimate: u64,
    #[serde(skip)]
    pub media_ids: Vec<Uuid>,
}

#[derive(Clone)]
pub struct DownloadPlan {
    pub id: String,
    pub parts: Vec<DownloadPart>,
    pub expires_at: Instant,
}

// App State
#[derive(Clone)]
pub struct AppState {
    pub upload_use_case: Arc<UploadMediaUseCase>,
    pub search_use_case: Arc<SearchSimilarUseCase>,
    pub list_use_case: Arc<ListMediaUseCase>,
    pub delete_use_case: Arc<DeleteMediaUseCase>,
    pub group_use_case: Arc<GroupMediaUseCase>,
    pub tag_learning_use_case: Arc<TagLearningUseCase>,
    pub repo: Arc<dyn MediaRepository>,
    pub upload_dir: PathBuf,
    pub auth_config: Option<AuthConfig>,
    pub upload_semaphore: Arc<Semaphore>,
    pub login_rate_limiter: Arc<Mutex<HashMap<IpAddr, (u32, Instant)>>>,
    pub download_plans: Arc<Mutex<HashMap<String, DownloadPlan>>>,
}


#[derive(Deserialize)]
pub struct Pagination {
    pub page: Option<usize>,
    pub limit: Option<usize>,
    pub media_type: Option<String>,
    pub favorite: Option<bool>,
    pub tags: Option<String>, // Comma-separated
    /// Sort direction: "asc" or "desc" (default "desc")
    pub sort: Option<String>,
    /// Sort field: "date" or "size" (default "date")
    pub sort_by: Option<String>,
}

async fn list_handler(
    State(state): State<AppState>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, DomainError> {
    let page = pagination.page.unwrap_or(1);
    let limit = pagination.limit.unwrap_or(20).min(MAX_PAGE_LIMIT);

    // Ensure valid page
    let page = if page < 1 { 1 } else { page };

    let sort_asc = pagination.sort.as_deref() == Some("asc");
    let sort_by = pagination.sort_by.as_deref().unwrap_or("date");
    let favorite = pagination.favorite.unwrap_or(false);

    let tags = pagination.tags.as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect());

    let results = state.list_use_case.execute(page, limit, pagination.media_type.as_deref(), favorite, tags, sort_asc, sort_by).await?;

    Ok(Json(results))
}

/// Sanitize a filename: strip path separators, control chars, quotes; fallback to "download" if empty.
fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| {
            !c.is_control()
                && *c != '/'
                && *c != '\\'
                && *c != '"'
                && *c != '\''
                && *c != '\0'
        })
        .collect();
    let sanitized = sanitized.trim().to_string();
    if sanitized.is_empty() {
        "download".to_string()
    } else {
        sanitized
    }
}

// Error handling
impl IntoResponse for DomainError {
    fn into_response(self) -> axum::response::Response {
        // Log the error for debugging
        match &self {
            DomainError::DuplicateMedia | DomainError::NotFound => {},
            DomainError::Database(e) => error!("Database Error: {}", e),
            DomainError::Ai(e) => error!("AI Error: {}", e),
            DomainError::Hashing(e) => error!("Hashing Error: {}", e),
            DomainError::Io(e) => error!("IO Error: {}", e),
            DomainError::ModelLoad(e) => error!("Model Load Error: {}", e),
        }

        let (status, message) = match self {
            DomainError::DuplicateMedia => (StatusCode::CONFLICT, "Media already exists".to_string()),
            DomainError::NotFound => (StatusCode::NOT_FOUND, "Media not found".to_string()),
            DomainError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string()),
            DomainError::Ai(e) => (StatusCode::BAD_REQUEST, e), // AI errors are usually client-data-related (not enough examples)
            DomainError::Hashing(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string()),
            DomainError::Io(e) => {
                // Keep user-facing messages, genericize internal ones
                let user_facing_prefixes = ["No file", "Folder", "Too many", "File type"];
                if user_facing_prefixes.iter().any(|p| e.starts_with(p)) {
                    (StatusCode::INTERNAL_SERVER_ERROR, e)
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
                }
            },
            DomainError::ModelLoad(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string()),
        };


        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

pub fn app_router(state: AppState) -> Router {
    // Auth-related routes (always accessible, no auth middleware)
    let auth_routes = Router::new()
        .route("/login", post(login_handler))
        .route("/logout", post(logout_handler))
        .route("/auth-check", get(auth_check_handler))
        .with_state(state.clone());

    // Protected API routes
    let protected_routes = Router::new()
        .route("/upload", post(upload_handler))
        .route("/search", post(search_handler))
        .route("/media", get(list_handler))
        .route("/media/batch-delete", post(batch_delete_handler))
        .route("/media/download", post(batch_download_handler))
        .route("/media/download/plan", post(batch_download_plan_handler))
        .route("/media/download/stream/{part_id}", get(batch_download_stream_handler))
        .route("/media/group", post(group_media_handler))

        .route("/media/{id}", get(get_media_handler).delete(delete_handler))
        .route("/media/{id}/favorite", post(toggle_favorite_handler))
        .route("/media/{id}/tags", put(update_tags_handler))
        .route("/media/batch-tags", put(batch_update_tags_handler))
        .route("/media/{id}/similar", get(search_by_id_handler))
        .route("/tags", get(list_tags_handler))
        .route("/tags/models", get(list_trained_tags_handler))
        .route("/tags/count", get(get_auto_tags_count_handler))
        .route("/tags/learn", post(learn_tag_handler))
        .route("/tags/auto-tag", post(auto_tag_handler))
        .route("/tags/{id}/apply", post(apply_tag_handler))
        .route("/stats", get(stats_handler))

        .route("/folders", get(list_folders_handler).post(create_folder_handler))
        .route("/folders/reorder", put(reorder_folders_handler))
        .route("/folders/{id}", put(rename_folder_handler).delete(delete_folder_handler))
        .route("/folders/{id}/media", get(list_folder_media_handler).post(add_to_folder_handler))
        .route("/folders/{id}/media/remove", post(remove_from_folder_handler))
        .route("/folders/{id}/download", get(download_folder_handler))
        .with_state(state.clone());

    // If auth is enabled, wrap protected routes with middleware
    let protected_routes = if let Some(ref config) = state.auth_config {
        let config = config.clone();
        protected_routes.layer(axum::middleware::from_fn(
            crate::presentation::auth::require_auth,
        )).layer(axum::Extension(config))
    } else {
        protected_routes
    };

    Router::new()
        .merge(auth_routes)
        .merge(protected_routes)
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

async fn login_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    // Rate limiting by IP
    {
        let ip = addr.ip();
        let mut limiter = state.login_rate_limiter.lock().await;
        let now = Instant::now();

        let entry = limiter.entry(ip).or_insert((0, now));
        if now.duration_since(entry.1).as_secs() > RATE_LIMIT_WINDOW_SECS {
            // Window expired, reset
            *entry = (0, now);
        }

        if entry.0 >= MAX_LOGIN_ATTEMPTS {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({ "error": "Too many login attempts. Try again later." })),
            )
                .into_response();
        }

        entry.0 += 1;
    }

    match &state.auth_config {
        Some(config) => {
            if config.verify_password(&body.password) {
                let token = config.generate_token();
                let cookie = format!(
                    "gallery_session={}; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age={}",
                    token,
                    60 * 60 * 24 * 30 // 30 days
                );
                (
                    StatusCode::OK,
                    [(header::SET_COOKIE, cookie)],
                    Json(json!({ "ok": true })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Invalid password" })),
                )
                    .into_response()
            }
        }
        None => {
            // No auth configured — always succeed
            (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
        }
    }
}

async fn logout_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Invalidate all sessions
    if let Some(ref config) = state.auth_config {
        config.invalidate_sessions();
    }
    let cookie = "gallery_session=; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=0";
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie.to_string())],
        Json(json!({ "ok": true })),
    )
}

async fn auth_check_handler(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    match &state.auth_config {
        Some(config) => {
            // Check the cookie manually
            let token = req
                .headers()
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| {
                    cookies.split(';').find_map(|part| {
                        let part = part.trim();
                        part.strip_prefix("gallery_session=").map(|v| v.to_string())
                    })
                });
            if let Some(token) = token {
                if config.verify_token(&token) {
                    return (StatusCode::OK, Json(json!({ "authenticated": true, "required": true }))).into_response();
                }
            }
            (StatusCode::UNAUTHORIZED, Json(json!({ "authenticated": false, "required": true }))).into_response()
        }
        None => {
            // No auth configured
            (StatusCode::OK, Json(json!({ "authenticated": true, "required": false }))).into_response()
        }
    }
}

async fn stats_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, DomainError> {
    let counts = state.repo.media_counts()?;

    // Disk space for the volume containing the upload directory
    let (disk_free_bytes, disk_total_bytes) = get_disk_space(&state.upload_dir);

    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "total_files": counts.total,
        "total_images": counts.images,
        "total_videos": counts.videos,
        "total_size_bytes": counts.total_size_bytes,
        "disk_free_bytes": disk_free_bytes,
        "disk_total_bytes": disk_total_bytes,
    })))
}

/// Get free and total disk space for the volume containing `path`.
fn get_disk_space(path: &std::path::Path) -> (u64, u64) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut free_bytes: u64 = 0;
        let mut total_bytes: u64 = 0;
        unsafe {
            windows_disk_free(wide.as_ptr(), &mut free_bytes, &mut total_bytes);
        }
        (free_bytes, total_bytes)
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::ffi::CString;
        let c_path = CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let free = stat.f_bavail as u64 * stat.f_frsize as u64;
                let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                (free, total)
            } else {
                (0, 0)
            }
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_disk_free(path: *const u16, free: &mut u64, total: &mut u64) {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }
    let mut free_to_caller: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free: u64 = 0;
    if GetDiskFreeSpaceExW(path, &mut free_to_caller, &mut total_bytes, &mut total_free) != 0 {
        *free = free_to_caller;
        *total = total_bytes;
    }
}

#[derive(Serialize)]
struct UploadResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    media: Option<MediaItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    filename: String,
}

/// Stream a multipart field to a temp file chunk-by-chunk.
/// Returns (temp_path, total_bytes_written).
async fn stream_field_to_temp(
    mut field: axum::extract::multipart::Field<'_>,
) -> Result<(PathBuf, u64), DomainError> {
    let temp_dir = std::env::temp_dir();
    let temp_id = Uuid::new_v4();
    let temp_path = temp_dir.join(format!("gallerynet_upload_{}.tmp", temp_id));

    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| DomainError::Io(format!("Failed to create temp file: {}", e)))?;

    let mut total: u64 = 0;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| DomainError::Io(format!("Failed to read multipart chunk: {}", e)))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| DomainError::Io(format!("Failed to write temp file: {}", e)))?;
        total += chunk.len() as u64;
    }

    file.flush().await.map_err(|e| DomainError::Io(e.to_string()))?;
    Ok((temp_path, total))
}

async fn upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, DomainError> {
    // Collect all file fields — stream each to a temp file to avoid buffering in RAM
    let mut pending: Vec<(String, PathBuf)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| DomainError::Io(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            continue;
        }

        // Cap the number of files per upload
        if pending.len() >= MAX_UPLOAD_FILES {
            // Clean up already-created temp files
            for (_, path) in &pending {
                let _ = tokio::fs::remove_file(path).await;
            }
            return Err(DomainError::Io(format!(
                "Too many files in upload (max {})",
                MAX_UPLOAD_FILES
            )));
        }

        let filename = field.file_name().unwrap_or("unknown").to_string();
        let (temp_path, _size) = stream_field_to_temp(field).await?;
        pending.push((filename, temp_path));
    }

    if pending.is_empty() {
        return Err(DomainError::Io("No file uploaded".to_string()));
    }

    // Single file — return the MediaItem directly for backward compatibility
    if pending.len() == 1 {
        let (filename, temp_path) = pending.into_iter().next().unwrap();
        let data = tokio::fs::read(&temp_path).await.map_err(|e| DomainError::Io(e.to_string()))?;
        let _ = tokio::fs::remove_file(&temp_path).await;
        let media = state.upload_use_case.execute(filename, &data).await?;
        return Ok((StatusCode::CREATED, Json(serde_json::to_value(media).unwrap())));
    }

    // Multiple files — process concurrently, return array of results
    let mut handles = Vec::with_capacity(pending.len());
    for (filename, temp_path) in pending {
        let use_case = state.upload_use_case.clone();
        let semaphore = state.upload_semaphore.clone(); // Clone semaphore for the task

        handles.push(tokio::spawn(async move {
            // Acquire permit before heavy processing (reading file + image decode + ONNX)
            let _permit = semaphore.acquire().await.unwrap();

            let data = tokio::fs::read(&temp_path).await;
            let _ = tokio::fs::remove_file(&temp_path).await;
            let data = match data {
                Ok(d) => d,
                Err(e) => return UploadResult {
                    media: None,
                    error: Some(format!("Failed to read temp file: {}", e)),
                    filename,
                },
            };
            match use_case.execute(filename.clone(), &data).await {
                Ok(media) => UploadResult { media: Some(media), error: None, filename },
                Err(e) => UploadResult { media: None, error: Some(e.to_string()), filename },
            }
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(e) => results.push(UploadResult {
                media: None,
                error: Some(format!("Task panicked: {}", e)),
                filename: "unknown".to_string(),
            }),
        }
    }

    Ok((StatusCode::CREATED, Json(serde_json::to_value(results).unwrap())))
}

async fn search_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, DomainError> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut similarity: f32 = 0.0; // Default 0% similarity (accept everything up to distance 2.0)

    while let Some(field) = multipart.next_field().await.map_err(|e| DomainError::Io(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();

        if name == "file" {
            let data = field.bytes().await.map_err(|e| DomainError::Io(e.to_string()))?;
            file_bytes = Some(data.to_vec());
        } else if name == "similarity" {
            let val = field.text().await.map_err(|e| DomainError::Io(e.to_string()))?;
            similarity = val.parse().unwrap_or(0.0);
        }
    }

    if let Some(data) = file_bytes {
        let limit = 10;
        // Convert similarity (0-100) to max_distance (2.0 - 0.0)
        let max_distance = 2.0 * (1.0 - (similarity / 100.0));

        let results = state.search_use_case.execute(&data, limit, max_distance).await?;
        return Ok(Json(results));
    }

    Err(DomainError::Io("No file uploaded".to_string()))
}

#[derive(Deserialize)]
pub struct SimilarQuery {
    pub similarity: Option<f32>,
    pub limit: Option<usize>,
}

async fn search_by_id_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<SimilarQuery>,
) -> Result<impl IntoResponse, DomainError> {
    let similarity = params.similarity.unwrap_or(0.0);
    let limit = params.limit.unwrap_or(20).min(MAX_PAGE_LIMIT);

    // Convert similarity (0-100) to max_distance (2.0 - 0.0)
    let max_distance = 2.0 * (1.0 - (similarity / 100.0));
    // Clamp to valid range just in case
    let max_distance = max_distance.max(0.0).min(2.0);

    let results = state.search_use_case.execute_by_id(id, limit, max_distance).await?;
    Ok(Json(results))
}

async fn get_media_handler(
    State(state): State<AppState>,

    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, DomainError> {
    match state.repo.find_by_id(id)? {
        Some(item) => Ok(Json(serde_json::to_value(item).unwrap())),
        None => Err(DomainError::NotFound),
    }
}

async fn delete_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, DomainError> {
    state.delete_use_case.execute(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct FavoriteRequest {
    pub favorite: bool,
}

async fn toggle_favorite_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<FavoriteRequest>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.set_favorite(id, body.favorite)?;
    Ok(StatusCode::OK)
}

async fn list_tags_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, DomainError> {
    let tags = state.repo.get_all_tags()?;
    Ok(Json(tags))
}

async fn list_trained_tags_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, DomainError> {
    let tags = state.tag_learning_use_case.get_trainable_tags()?;
    Ok(Json(tags))
}

#[derive(Deserialize)]
pub struct CountAutoTagsQuery {
    pub folder_id: Option<Uuid>,
}

async fn get_auto_tags_count_handler(
    State(state): State<AppState>,
    Query(query): Query<CountAutoTagsQuery>,
) -> Result<impl IntoResponse, DomainError> {
    let count = state.repo.count_auto_tags(query.folder_id)?;
    Ok(Json(json!({ "count": count })))
}

#[derive(Deserialize)]
pub struct LearnTagRequest {
    pub tag_name: String,
}

async fn learn_tag_handler(
    State(state): State<AppState>,
    Json(body): Json<LearnTagRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let use_case = state.tag_learning_use_case.clone();
    let tag_name = body.tag_name.clone();
    let count = tokio::task::spawn_blocking(move || use_case.learn_tag(&tag_name))
        .await
        .map_err(|e| DomainError::Ai(e.to_string()))??;
    Ok(Json(json!({ "auto_tagged_count": count })))
}


#[derive(Deserialize)]
pub struct AutoTagRequest {
    pub folder_id: Option<Uuid>,
}

async fn auto_tag_handler(
    State(state): State<AppState>,
    Json(body): Json<AutoTagRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let use_case = state.tag_learning_use_case.clone();
    let folder_id = body.folder_id;
    let result = tokio::task::spawn_blocking(move || use_case.run_auto_tagging(folder_id))
        .await
        .map_err(|e| DomainError::Ai(e.to_string()))??;
    Ok(Json(json!({
        "before": result.before,
        "after": result.after,
        "models_processed": result.models_processed
    })))
}

#[derive(Deserialize)]
pub struct ApplyTagRequest {
    pub folder_id: Option<Uuid>,
}

async fn apply_tag_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ApplyTagRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let use_case = state.tag_learning_use_case.clone();
    let folder_id = body.folder_id;
    let count = tokio::task::spawn_blocking(move || use_case.apply_tag_model(id, folder_id))
        .await
        .map_err(|e| DomainError::Ai(e.to_string()))??;
    Ok(Json(json!({ "auto_tagged_count": count })))
}

#[derive(Deserialize)]
struct UpdateTagsRequest {
    tags: Vec<String>,
}

async fn update_tags_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTagsRequest>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.update_media_tags(id, body.tags)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct BatchUpdateTagsRequest {
    ids: Vec<Uuid>,
    tags: Vec<String>,
}

async fn batch_update_tags_handler(
    State(state): State<AppState>,
    Json(body): Json<BatchUpdateTagsRequest>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.update_media_tags_batch(&body.ids, &body.tags)?;
    Ok(StatusCode::OK)
}

fn create_download_plan<T: HasFilenames>(
    items: Vec<T>,
    upload_dir: &std::path::Path,
    base_name: &str,
) -> DownloadPlan {
    let entries = prepare_zip_entries(&items, upload_dir);
    
    let mut parts = Vec::new();
    let mut current_part_media_ids = Vec::new();
    let mut current_part_size: u64 = 0;
    
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    for (entry, item) in entries.into_iter().zip(items.iter()) {
        if !current_part_media_ids.is_empty() && current_part_size + entry.size > MAX_ZIP_BYTES {
            let part_id = Uuid::new_v4().to_string();
            parts.push(DownloadPart {
                id: part_id,
                filename: format!("{}_part{}_{}.zip", base_name, parts.len() + 1, timestamp),
                size_estimate: current_part_size,
                media_ids: current_part_media_ids,
            });
            current_part_media_ids = Vec::new();
            current_part_size = 0;
        }
        current_part_media_ids.push(item.id());
        current_part_size += entry.size;
    }
    
    if !current_part_media_ids.is_empty() {
        let part_id = Uuid::new_v4().to_string();
        let filename = if parts.is_empty() {
            format!("{}_{}.zip", base_name, timestamp)
        } else {
            format!("{}_part{}_{}.zip", base_name, parts.len() + 1, timestamp)
        };
        parts.push(DownloadPart {
            id: part_id,
            filename,
            size_estimate: current_part_size,
            media_ids: current_part_media_ids,
        });
    }

    let plan_id = Uuid::new_v4().to_string();
    DownloadPlan {
        id: plan_id,
        parts,
        expires_at: Instant::now() + std::time::Duration::from_secs(3600), // 1 hour
    }
}

async fn batch_download_plan_handler(
    State(state): State<AppState>,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    if ids.is_empty() {
        return Err(DomainError::Io("No files requested".to_string()));
    }

    // Deduplicate IDs
    let unique_ids: Vec<Uuid> = {
        let mut seen = std::collections::HashSet::new();
        ids.into_iter().filter(|id| seen.insert(*id)).collect()
    };

    // Look up all requested media items
    let mut items = Vec::new();
    for id in &unique_ids {
        if let Some(item) = state.repo.find_by_id(*id)? {
            items.push(item);
        }
    }

    if items.is_empty() {
        return Err(DomainError::NotFound);
    }

    let base_name = format!("gallerynet_{}", items.len());
    let plan = create_download_plan(items, &state.upload_dir, &base_name);
    let plan_id = plan.id.clone();
    let parts = plan.parts.clone();

    {
        let mut plans = state.download_plans.lock().await;
        let now = Instant::now();
        plans.retain(|_, p| p.expires_at > now);
        plans.insert(plan_id.clone(), plan);
    }

    Ok(Json(json!({
        "plan_id": plan_id,
        "parts": parts
    })))
}

async fn batch_download_stream_handler(
    State(state): State<AppState>,
    Path(part_id): Path<String>,
) -> Result<impl IntoResponse, DomainError> {
    let part = {
        let plans = state.download_plans.lock().await;
        plans.values()
            .flat_map(|p| p.parts.iter())
            .find(|p| p.id == part_id)
            .cloned()
            .ok_or(DomainError::NotFound)?
    };

    let mut items = Vec::new();
    for id in &part.media_ids {
        if let Some(item) = state.repo.find_by_id(*id)? {
            items.push(item);
        }
    }

    if items.is_empty() {
        return Err(DomainError::NotFound);
    }

    let entries = prepare_zip_entries(&items, &state.upload_dir);
    let items_count = items.len();
    let filename = part.filename.clone();

    let (writer, reader) = tokio::io::duplex(16 * 1024 * 1024); // 16MB buffer
    
    tokio::spawn(async move {
        let mut zip = ZipFileWriter::new(writer.compat_write());
        
        info!("Streaming zip download start: {} ({} items)", filename, entries.len());
        
        for (i, entry) in entries.into_iter().enumerate() {
            if i % 50 == 0 || i == items_count - 1 {
                info!("[{}/{}] Streaming file: {}", i + 1, items_count, entry.zip_name);
            }
            
            let builder = ZipEntryBuilder::new(entry.zip_name.clone().into(), Compression::Stored);
            match tokio::fs::File::open(&entry.disk_path).await {
                Ok(file) => {
                    match zip.write_entry_stream(builder).await {
                        Ok(mut entry_writer) => {
                            let mut buf_file = tokio::io::BufReader::with_capacity(128 * 1024, file);
                            if let Err(e) = tokio::io::copy(&mut buf_file, &mut (&mut entry_writer).compat_write()).await {
                                if e.kind() == std::io::ErrorKind::BrokenPipe {
                                    info!("Client disconnected, aborting download: {}", filename);
                                    return;
                                }
                                error!("Failed to stream entry {}: {}", entry.zip_name, e);
                            }
                            let _ = entry_writer.close().await;
                        }
                        Err(e) => {
                            error!("Failed to start entry {}: {}", entry.zip_name, e);
                            return; 
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to open {} for streaming: {}", entry.disk_path.display(), e);
                }
            }
        }
        
        if let Err(e) = zip.close().await {
            error!("Failed to close zip stream: {}", e);
        }
        
        info!("Streaming zip download finished: {}", filename);
    });

    let stream = tokio_util::io::ReaderStream::new(reader);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", part.filename).parse().unwrap()
    );

    Ok((headers, body))
}

async fn batch_delete_handler(

    State(state): State<AppState>,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    let deleted = state.delete_use_case.execute_batch(&ids).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

/// Represents a file to include in a zip download.
struct ZipEntry {
    /// Sanitized, deduplicated filename for inside the archive.
    zip_name: String,
    /// Path on disk to read from.
    disk_path: PathBuf,
    /// File size in bytes (used for splitting).
    size: u64,
}


/// Stream a zip archive of the given items incrementally using async_zip.
async fn stream_zip_response<I>(
    items: I,
    upload_dir: PathBuf,
    outer_name: String,
) -> Result<axum::response::Response, DomainError>
where
    I: IntoIterator + Send + 'static,
    I::Item: HasFilenames + Send + 'static,
{
    let items_vec: Vec<I::Item> = items.into_iter().collect();
    let name_for_log = outer_name.clone();
    
    let entries = prepare_zip_entries(items_vec, &upload_dir);
    let items_count = entries.len();

    let (writer, reader) = tokio::io::duplex(16 * 1024 * 1024); // 16MB buffer
    
    tokio::spawn(async move {
        let mut zip = ZipFileWriter::new(writer.compat_write());
        info!("Streaming zip start: {} ({} items)", name_for_log, items_count);
        
        for (i, entry) in entries.into_iter().enumerate() {
            if i % 50 == 0 || i == items_count - 1 {
                info!("[{}/{}] Streaming: {}", i + 1, items_count, entry.zip_name);
            }
            let builder = ZipEntryBuilder::new(entry.zip_name.clone().into(), Compression::Stored);
            match tokio::fs::File::open(&entry.disk_path).await {
                Ok(file) => {
                    match zip.write_entry_stream(builder).await {
                        Ok(mut entry_writer) => {
                            let mut buf_file = tokio::io::BufReader::with_capacity(128 * 1024, file);
                            if let Err(e) = tokio::io::copy(&mut buf_file, &mut (&mut entry_writer).compat_write()).await {
                                if e.kind() == std::io::ErrorKind::BrokenPipe {
                                    info!("Client disconnected, aborting zip stream: {}", name_for_log);
                                    return;
                                }
                                error!("Failed to stream entry {}: {}", entry.zip_name, e);
                            }
                            let _ = entry_writer.close().await;
                        }
                        Err(e) => {
                            error!("Failed to start entry {}: {}", entry.zip_name, e);
                            return;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to open {} for streaming: {}", entry.disk_path.display(), e);
                }
            }
        }

        
        if let Err(e) = zip.close().await {
            error!("Failed to close zip stream: {}", e);
        }
        info!("Streaming zip finished: {}", name_for_log);
    });

    let stream = tokio_util::io::ReaderStream::new(reader);
    let body = Body::from_stream(stream);
    
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", outer_name).parse().unwrap()
    );
    // Note: Content-Length for a ZIP stream is not easily pre-calculable 
    // without actually zipping everything (due to headers and compression),
    // so we omit it for true incremental streaming.

    Ok((headers, body).into_response())
}

/// Prepare `ZipEntry` list from items, handling filename sanitization and deduplication.
fn prepare_zip_entries<I>(items: I, upload_dir: &std::path::Path) -> Vec<ZipEntry>
where
    I: IntoIterator,
    I::Item: HasFilenames,
{
    let mut used_names = std::collections::HashMap::<String, usize>::new();
    items
        .into_iter()
        .map(|item| {
            let base_name = sanitize_filename(item.original_filename());
            let entry = used_names.entry(base_name.clone()).or_insert(0);
            let zip_name = if *entry == 0 {
                base_name.clone()
            } else {
                let dot = base_name.rfind('.').unwrap_or(base_name.len());
                format!("{}_{}{}", &base_name[..dot], entry, &base_name[dot..])
            };
            *entry += 1;

            let disk_path = upload_dir.join(item.disk_filename());
            let size = disk_path.metadata().map(|m| m.len()).unwrap_or(0);
            ZipEntry { zip_name, disk_path, size }
        })
        .collect()
}

/// Trait to abstract over MediaItem and MediaSummary for zip entry preparation.
trait HasFilenames {
    fn original_filename(&self) -> &str;
    fn disk_filename(&self) -> &str;
    fn id(&self) -> Uuid;
}

impl HasFilenames for MediaItem {
    fn original_filename(&self) -> &str { &self.original_filename }
    fn disk_filename(&self) -> &str { &self.filename }
    fn id(&self) -> Uuid { self.id }
}

impl HasFilenames for crate::domain::MediaSummary {
    fn original_filename(&self) -> &str { &self.original_filename }
    fn disk_filename(&self) -> &str { &self.filename }
    fn id(&self) -> Uuid { self.id }
}

impl<T: HasFilenames> HasFilenames for &T {
    fn original_filename(&self) -> &str { (*self).original_filename() }
    fn disk_filename(&self) -> &str { (*self).disk_filename() }
    fn id(&self) -> Uuid { (*self).id() }
}

async fn batch_download_handler(

    State(state): State<AppState>,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    if ids.is_empty() {
        return Err(DomainError::Io("No files requested".to_string()));
    }

    // Deduplicate IDs
    let unique_ids: Vec<Uuid> = {
        let mut seen = std::collections::HashSet::new();
        ids.into_iter().filter(|id| seen.insert(*id)).collect()
    };

    // Look up all requested media items
    let mut items = Vec::new();
    for id in &unique_ids {
        if let Some(item) = state.repo.find_by_id(*id)? {
            items.push(item);
        }
    }

    if items.is_empty() {
        return Err(DomainError::NotFound);
    }

    // Single file — serve directly without zipping
    if items.len() == 1 {
        let item = &items[0];
        let file_path = state.upload_dir.join(&item.filename);
        let file = tokio::fs::File::open(&file_path)
            .await
            .map_err(|e| DomainError::Io(e.to_string()))?;
        
        let metadata = file.metadata().await
            .map_err(|e| DomainError::Io(e.to_string()))?;
        let size = metadata.len();

        let content_type = mime_guess::from_path(&item.original_filename)
            .first_or_octet_stream()
            .to_string();

        let safe_name = sanitize_filename(&item.original_filename);
        let headers = [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", safe_name),
            ),
            (header::CONTENT_LENGTH, size.to_string()),
        ];
        return Ok((headers, Body::from_stream(ReaderStream::new(file))).into_response());
    }


    // Multiple files — build zip archive(s), splitting by size
    let file_count = items.len();
    let upload_dir = state.upload_dir.clone();
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let outer_name = format!("gallerynet_{}_{}.zip", file_count, timestamp);

    Ok(stream_zip_response(items, upload_dir, outer_name).await?)
}


#[derive(Deserialize)]
pub struct GroupRequest {
    pub folder_id: Option<Uuid>,
    pub threshold: Option<f32>, // Distance threshold (0.0 - 2.0)
    pub similarity: Option<f32>, // Alternative: 0-100% similarity
}

async fn group_media_handler(
    State(state): State<AppState>,
    Json(body): Json<GroupRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let mut threshold = 0.2; // Default distance: fairly similar

    if let Some(t) = body.threshold {
        threshold = t;
    } else if let Some(s) = body.similarity {
        // Convert 0-100 similarity to distance
        threshold = 2.0 * (1.0 - (s / 100.0));
    }

    // Clamp
    threshold = threshold.max(0.0).min(2.0);

    let groups = state.group_use_case.execute(body.folder_id, threshold).await?;
    Ok(Json(groups))
}

// ==================== Folder endpoints ====================

#[derive(Deserialize)]
struct CreateFolderRequest {
    name: String,
}

async fn create_folder_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateFolderRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(DomainError::Io("Folder name cannot be empty".to_string()));
    }
    let id = Uuid::new_v4();
    let folder = state.repo.create_folder(id, name)?;
    Ok((StatusCode::CREATED, Json(folder)))
}

async fn list_folders_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, DomainError> {
    let folders = state.repo.list_folders()?;
    Ok(Json(folders))
}

async fn delete_folder_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.delete_folder(id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn rename_folder_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateFolderRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(DomainError::Io("Folder name cannot be empty".to_string()));
    }
    state.repo.rename_folder(id, name)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Accepts an ordered array of folder IDs and sets their sort_order accordingly.
async fn reorder_folders_handler(
    State(state): State<AppState>,
    Json(folder_ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    let order: Vec<(Uuid, i64)> = folder_ids
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i as i64))
        .collect();
    state.repo.reorder_folders(&order)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct FolderPagination {
    page: Option<usize>,
    limit: Option<usize>,
    media_type: Option<String>,
    sort: Option<String>,
    /// Sort field: "date" or "size" (default "date")
    sort_by: Option<String>,
    favorite: Option<bool>,
    tags: Option<String>,
}

async fn list_folder_media_handler(
    State(state): State<AppState>,
    Path(folder_id): Path<Uuid>,
    Query(pagination): Query<FolderPagination>,
) -> Result<impl IntoResponse, DomainError> {
    let page = pagination.page.unwrap_or(1).max(1);
    let limit = pagination.limit.unwrap_or(20).min(MAX_PAGE_LIMIT);
    let offset = (page - 1) * limit;
    let sort_asc = pagination.sort.as_deref() == Some("asc");
    let sort_by = pagination.sort_by.as_deref().unwrap_or("date");
    let favorite = pagination.favorite.unwrap_or(false);

    let tags = pagination.tags.as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect());

    let results = state.repo.find_all_in_folder(folder_id, limit, offset, pagination.media_type.as_deref(), favorite, tags, sort_asc, sort_by)?;
    Ok(Json(results))
}

async fn add_to_folder_handler(
    State(state): State<AppState>,
    Path(folder_id): Path<Uuid>,
    Json(media_ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    let added = state.repo.add_media_to_folder(folder_id, &media_ids)?;
    Ok(Json(json!({ "added": added })))
}

#[derive(Deserialize)]
struct RemoveFromFolderRequest {
    media_ids: Vec<Uuid>,
}

async fn remove_from_folder_handler(
    State(state): State<AppState>,
    Path(folder_id): Path<Uuid>,
    Json(body): Json<RemoveFromFolderRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let removed = state.repo.remove_media_from_folder(folder_id, &body.media_ids)?;
    Ok(Json(json!({ "removed": removed })))
}

async fn download_folder_handler(
    State(state): State<AppState>,
    Path(folder_id): Path<Uuid>,
) -> Result<impl IntoResponse, DomainError> {
    // Get folder details (for name)
    let folder = state.repo.get_folder(folder_id)?
        .ok_or(DomainError::NotFound)?;

    // Get all media in the folder
    let items = state.repo.get_folder_media_files(folder_id)?;

    if items.is_empty() {
        return Err(DomainError::Io("Folder is empty".to_string()));
    }

    let file_count = items.len();
    let upload_dir = state.upload_dir.clone();

    // Sanitize folder name for the zip filename
    let safe_name: String = folder.name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_name = safe_name.trim_matches(|c| c == '_').to_string();
    let safe_name = if safe_name.is_empty() { "folder".to_string() } else { safe_name };

    let plan = create_download_plan(items, &state.upload_dir, &safe_name);
    let plan_id = plan.id.clone();
    let parts = plan.parts.clone();

    {
        let mut plans = state.download_plans.lock().await;
        let now = Instant::now();
        plans.retain(|_, p| p.expires_at > now);
        plans.insert(plan_id.clone(), plan);
    }

    Ok(Json(json!({
        "plan_id": plan_id,
        "parts": parts
    })))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_path_traversal() {
        assert_eq!(sanitize_filename("../../../etc/passwd"), "......etcpasswd");
        assert_eq!(sanitize_filename("..\\..\\windows\\system32"), "....windowssystem32");
    }

    #[test]
    fn sanitize_filename_quotes_and_control_chars() {
        assert_eq!(sanitize_filename("file\"name.jpg"), "filename.jpg");
        assert_eq!(sanitize_filename("file'name.jpg"), "filename.jpg");
        assert_eq!(sanitize_filename("file\x00name.jpg"), "filename.jpg");
        assert_eq!(sanitize_filename("file\nname.jpg"), "filename.jpg");
    }

    #[test]
    fn sanitize_filename_empty() {
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("   "), "download");
        assert_eq!(sanitize_filename("/"), "download");
    }

    #[test]
    fn limit_capping() {


        assert_eq!(250_usize.min(MAX_PAGE_LIMIT), MAX_PAGE_LIMIT);
        assert_eq!(50_usize.min(MAX_PAGE_LIMIT), 50);
        assert_eq!(200_usize.min(MAX_PAGE_LIMIT), 200);
    }

    #[test]
    fn max_upload_files_constant_valid() {
        assert!(MAX_UPLOAD_FILES > 0);
        assert!(MAX_UPLOAD_FILES <= 10_000);
    }

    #[test]
    fn error_message_sanitization_database() {
        let err = DomainError::Database("SQLITE_ERROR: table foo has 5 columns but 3 values".to_string());
        let response = err.into_response();
        // Should be 500
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn error_message_sanitization_io_user_facing() {
        // These messages should be preserved
        let err = DomainError::Io("No file uploaded".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let err = DomainError::Io("Folder is empty".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let err = DomainError::Io("Too many files in upload (max 100)".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let err = DomainError::Io("File type not allowed: .exe".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn rate_limiter_constants_valid() {
        assert!(MAX_LOGIN_ATTEMPTS > 0);
        assert!(MAX_LOGIN_ATTEMPTS <= 100);
        assert!(RATE_LIMIT_WINDOW_SECS > 0);
        assert!(RATE_LIMIT_WINDOW_SECS <= 3600);
    }
}
