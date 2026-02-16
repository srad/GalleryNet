mod domain;
mod application;
mod infrastructure;
mod presentation;

use std::sync::Arc;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use infrastructure::{SqliteRepository, OrtProcessor, PhashGenerator};
use application::{UploadMediaUseCase, SearchSimilarUseCase, ListMediaUseCase, DeleteMediaUseCase, GroupMediaUseCase, TagLearningUseCase};
use presentation::{AppState, AuthConfig, app_router};

// UPDATED: Added ServeFile
use tower_http::services::{ServeDir, ServeFile};
use tower_http::cors::CorsLayer;
use axum::extract::DefaultBodyLimit;
// UPDATED: Added Router
use axum::Router;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Configuration
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "gallery.db".to_string());
    let model_path = std::env::var("MODEL_PATH").unwrap_or_else(|_| "assets/models/mobilenetv3.onnx".to_string());
    let upload_dir = PathBuf::from(std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string()));
    let thumbnail_dir = PathBuf::from(std::env::var("THUMBNAIL_DIR").unwrap_or_else(|_| "thumbnails".to_string()));
    let port = 3000;

    // Authentication — optional, enabled when GALLERY_PASSWORD is set
    let auth_config = std::env::var("GALLERY_PASSWORD").ok().and_then(|pw| {
        let pw = pw.trim().to_string();
        if pw.is_empty() {
            None
        } else {
            println!("Authentication enabled (GALLERY_PASSWORD is set)");
            Some(AuthConfig::new(pw))
        }
    });
    if auth_config.is_none() {
        println!("Warning: No GALLERY_PASSWORD set — running without authentication");
    }

    // Ensure directories exist
    if !upload_dir.exists() {
        std::fs::create_dir_all(&upload_dir).expect("Failed to create upload directory");
    }
    if !thumbnail_dir.exists() {
        std::fs::create_dir_all(&thumbnail_dir).expect("Failed to create thumbnail directory");
    }

    // Initialize Infrastructure
    println!("Initializing Database...");
    let repo = Arc::new(SqliteRepository::new(&db_path)?);

    println!("Initializing AI Processor (Loading {})...", model_path);
    let ai = match OrtProcessor::new(&model_path) {
        Ok(processor) => Arc::new(processor),
        Err(e) => {
            eprintln!("Warning: Failed to load AI model: {}. AI features will fail.", e);
            return Err(e.into());
        }
    };

    println!("Initializing Hasher...");
    let hasher = Arc::new(PhashGenerator::new());

    // Initialize Use Cases
    let upload_use_case = Arc::new(UploadMediaUseCase::new(
        repo.clone(),
        ai.clone(),
        hasher.clone(),
        upload_dir.clone(),
        thumbnail_dir.clone(),
    ));

    let search_use_case = Arc::new(SearchSimilarUseCase::new(
        repo.clone(),
        ai.clone(),
    ));

    let list_use_case = Arc::new(ListMediaUseCase::new(
        repo.clone(),
    ));

    let delete_use_case = Arc::new(DeleteMediaUseCase::new(
        repo.clone(),
        upload_dir.clone(),
        thumbnail_dir.clone(),
    ));

    let group_use_case = Arc::new(GroupMediaUseCase::new(
        repo.clone(),
    ));

    let tag_learning_use_case = Arc::new(TagLearningUseCase::new(
        repo.clone(),
    ));

    // Initialize App State
    let state = AppState {
        upload_use_case,
        search_use_case,
        list_use_case,
        delete_use_case,
        group_use_case,
        tag_learning_use_case,
        repo: repo.clone(),
        upload_dir: upload_dir.clone(),
        auth_config: auth_config.clone(),
        upload_semaphore: Arc::new(tokio::sync::Semaphore::new(2)), // Limit concurrent heavy uploads to 2
    };

    // --- UPDATED ROUTING ARCHITECTURE ---

    // 1. Group all backend logic (your existing routes) and attach the body limit
    let api_routes = app_router(state)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024 * 1024)); // 10GB

    // 2. Configure the React SPA fallback (always accessible — it serves the login page too)
    let serve_react_app = ServeDir::new("frontend/dist")
        .not_found_service(ServeFile::new("frontend/dist/index.html"));

    // 3. Static file routes for media — must be auth-protected
    let static_uploads = Router::new()
        .nest_service("/uploads", ServeDir::new(upload_dir))
        .nest_service("/thumbnails", ServeDir::new(thumbnail_dir))
        .layer(axum::middleware::map_response(|mut response: axum::response::Response| async move {
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            response
        }));

    // Wrap static routes with auth middleware if password is set
    let static_uploads = if let Some(ref config) = auth_config {
        static_uploads
            .layer(axum::middleware::from_fn(
                presentation::auth::require_auth,
            ))
            .layer(axum::Extension(config.clone()))
    } else {
        static_uploads
    };

    // 4. Assemble the final application router
    let app = Router::new()
        .nest("/api", api_routes) // All your endpoints now live under /api
        .merge(static_uploads)     // Auth-protected static file serving
        .layer(CorsLayer::permissive())
        .fallback_service(serve_react_app); // Anything else returns the React App (login page)

    // ------------------------------------

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    println!("Server running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await?;

    Ok(())
}