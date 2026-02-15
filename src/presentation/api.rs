use axum::{
    extract::{Multipart, State, Query, Path},
    http::{StatusCode, header},
    response::{Json, IntoResponse},
    routing::{get, post, put},
    Router,
};
use serde_json::json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use std::path::PathBuf;
use tracing::error;
use uuid::Uuid;
use tokio::io::AsyncWriteExt;

use crate::application::{UploadMediaUseCase, SearchSimilarUseCase, ListMediaUseCase, DeleteMediaUseCase, GroupMediaUseCase};
use crate::domain::{DomainError, MediaItem, MediaRepository};
use crate::presentation::auth::AuthConfig;

// App State
#[derive(Clone)]
pub struct AppState {
    pub upload_use_case: Arc<UploadMediaUseCase>,
    pub search_use_case: Arc<SearchSimilarUseCase>,
    pub list_use_case: Arc<ListMediaUseCase>,
    pub delete_use_case: Arc<DeleteMediaUseCase>,
    pub group_use_case: Arc<GroupMediaUseCase>,
    pub repo: Arc<dyn MediaRepository>,
    pub upload_dir: PathBuf,
    pub auth_config: Option<AuthConfig>,
    pub upload_semaphore: Arc<Semaphore>,
}

#[derive(Deserialize)]
pub struct Pagination {
    pub page: Option<usize>,
    pub limit: Option<usize>,
    pub media_type: Option<String>,
    pub favorite: Option<bool>,
    pub tags: Option<String>, // Comma-separated
    /// Sort direction for original_date: "asc" or "desc" (default "desc")
    pub sort: Option<String>,
}

async fn list_handler(
    State(state): State<AppState>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, DomainError> {
    let page = pagination.page.unwrap_or(1);
    let limit = pagination.limit.unwrap_or(20);
    
    // Ensure valid page
    let page = if page < 1 { 1 } else { page };
    
    let sort_asc = pagination.sort.as_deref() == Some("asc");
    let favorite = pagination.favorite.unwrap_or(false);
    
    let tags = pagination.tags.as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect());

    let results = state.list_use_case.execute(page, limit, pagination.media_type.as_deref(), favorite, tags, sort_asc).await?;
    
    Ok(Json(results))
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
            DomainError::DuplicateMedia => (StatusCode::CONFLICT, "Media already exists"),
            DomainError::NotFound => (StatusCode::NOT_FOUND, "Media not found"),
            DomainError::Database(_e) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
            DomainError::Ai(_e) => (StatusCode::INTERNAL_SERVER_ERROR, "AI processing error"),
            DomainError::Hashing(_e) => (StatusCode::INTERNAL_SERVER_ERROR, "Hashing error"),
            DomainError::Io(_e) => (StatusCode::INTERNAL_SERVER_ERROR, "IO error"),
            DomainError::ModelLoad(_e) => (StatusCode::INTERNAL_SERVER_ERROR, "Model loading error"),
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
        .route("/media/group", post(group_media_handler))
        .route("/media/{id}", get(get_media_handler).delete(delete_handler))
        .route("/media/{id}/favorite", post(toggle_favorite_handler))
        .route("/media/{id}/tags", put(update_tags_handler))
        .route("/media/batch-tags", put(batch_update_tags_handler))
        .route("/media/{id}/similar", get(search_by_id_handler))
        .route("/tags", get(list_tags_handler))
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
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    match &state.auth_config {
        Some(config) => {
            if body.password == config.password {
                let token = config.generate_token();
                let cookie = format!(
                    "gallery_session={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
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

async fn logout_handler() -> impl IntoResponse {
    let cookie = "gallery_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0";
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
    let limit = params.limit.unwrap_or(20);
    
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

async fn batch_delete_handler(
    State(state): State<AppState>,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    let deleted = state.delete_use_case.execute_batch(&ids).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

async fn batch_download_handler(
    State(state): State<AppState>,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<impl IntoResponse, DomainError> {
    if ids.is_empty() {
        return Err(DomainError::Io("No files requested".to_string()));
    }

    // Look up all requested media items
    let mut items = Vec::new();
    for id in &ids {
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
        let data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| DomainError::Io(e.to_string()))?;

        let content_type = mime_guess::from_path(&item.original_filename)
            .first_or_octet_stream()
            .to_string();

        let headers = [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", item.original_filename),
            ),
        ];
        return Ok((headers, data).into_response());
    }

    // Multiple files — create a zip archive in memory
    let file_count = items.len();
    let upload_dir = state.upload_dir.clone();
    let buf = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, DomainError> {
        use std::io::{Cursor, Write};
        let mut zip_buf = Vec::new();
        {
            let cursor = Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            // Track filenames to avoid duplicates in the archive
            let mut used_names = std::collections::HashMap::<String, usize>::new();

            for item in &items {
                let file_path = upload_dir.join(&item.filename);
                let data = std::fs::read(&file_path)
                    .map_err(|e| DomainError::Io(format!("{}: {}", item.filename, e)))?;

                // Deduplicate filenames within the zip
                let base_name = item.original_filename.clone();
                let entry = used_names.entry(base_name.clone()).or_insert(0);
                let zip_name = if *entry == 0 {
                    base_name.clone()
                } else {
                    let dot = base_name.rfind('.').unwrap_or(base_name.len());
                    format!("{}_{}{}", &base_name[..dot], entry, &base_name[dot..])
                };
                *entry += 1;

                zip.start_file(zip_name, options)
                    .map_err(|e| DomainError::Io(e.to_string()))?;
                zip.write_all(&data)
                    .map_err(|e| DomainError::Io(e.to_string()))?;
            }
            zip.finish().map_err(|e| DomainError::Io(e.to_string()))?;
        }
        Ok(zip_buf)
    })
    .await
    .map_err(|e| DomainError::Io(format!("Task failed: {}", e)))??;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let zip_name = format!("gallerynet_{}_{}.zip", file_count, timestamp);
    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", zip_name),
        ),
    ];
    Ok((headers, buf).into_response())
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
    favorite: Option<bool>,
    tags: Option<String>,
}

async fn list_folder_media_handler(
    State(state): State<AppState>,
    Path(folder_id): Path<Uuid>,
    Query(pagination): Query<FolderPagination>,
) -> Result<impl IntoResponse, DomainError> {
    let page = pagination.page.unwrap_or(1).max(1);
    let limit = pagination.limit.unwrap_or(20);
    let offset = (page - 1) * limit;
    let sort_asc = pagination.sort.as_deref() == Some("asc");
    let favorite = pagination.favorite.unwrap_or(false);
    
    let tags = pagination.tags.as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect());

    let results = state.repo.find_all_in_folder(folder_id, limit, offset, pagination.media_type.as_deref(), favorite, tags, sort_asc)?;
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

    // Reuse the zip logic from batch_download_handler
    let file_count = items.len();
    let upload_dir = state.upload_dir.clone();
    
    // We can't reuse the code directly without refactoring, so I'll duplicate the zip logic here
    // adapting it for MediaSummary (which has the fields we need: filename, original_filename)
    
    let buf = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, DomainError> {
        use std::io::{Cursor, Write};
        let mut zip_buf = Vec::new();
        {
            let cursor = Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            // Track filenames to avoid duplicates in the archive
            let mut used_names = std::collections::HashMap::<String, usize>::new();

            for item in &items {
                let file_path = upload_dir.join(&item.filename);
                // If file is missing, we skip or error? Batch download errors. Let's error for consistency.
                let data = std::fs::read(&file_path)
                    .map_err(|e| DomainError::Io(format!("{}: {}", item.filename, e)))?;

                // Deduplicate filenames within the zip
                let base_name = item.original_filename.clone();
                let entry = used_names.entry(base_name.clone()).or_insert(0);
                let zip_name = if *entry == 0 {
                    base_name.clone()
                } else {
                    let dot = base_name.rfind('.').unwrap_or(base_name.len());
                    format!("{}_{}{}", &base_name[..dot], entry, &base_name[dot..])
                };
                *entry += 1;

                zip.start_file(zip_name, options)
                    .map_err(|e| DomainError::Io(e.to_string()))?;
                zip.write_all(&data)
                    .map_err(|e| DomainError::Io(e.to_string()))?;
            }
            zip.finish().map_err(|e| DomainError::Io(e.to_string()))?;
        }
        Ok(zip_buf)
    })
    .await
    .map_err(|e| DomainError::Io(format!("Task failed: {}", e)))??;

    // Sanitize folder name
    // Allow unicode alphanumeric (for international support), plus basic safe punctuation.
    // Replace whitespace and other symbols/separators with underscores.
    let safe_name: String = folder.name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Trim underscores from ends just in case
    let safe_name = safe_name.trim_matches(|c| c == '_').to_string();
    let safe_name = if safe_name.is_empty() { "folder".to_string() } else { safe_name };

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let zip_name = format!("{}_{}_{}.zip", safe_name, file_count, timestamp);
    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", zip_name),
        ),
    ];
    Ok((headers, buf).into_response())
}
