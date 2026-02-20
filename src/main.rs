mod domain;
mod application;
mod infrastructure;
mod presentation;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::HashMap;

use std::net::SocketAddr;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use infrastructure::{SqliteRepository, OrtProcessor, PhashGenerator};
use domain::ports::MediaRepository;
use application::{UploadMediaUseCase, SearchSimilarUseCase, ListMediaUseCase, DeleteMediaUseCase, GroupMediaUseCase, GroupFacesUseCase, TagLearningUseCase, FixThumbnailsUseCase, ExternalSearchUseCase, IndexFacesUseCase, FindSimilarFacesUseCase, ListPeopleUseCase};
use presentation::{AppState, AuthConfig, app_router};


use tower_http::services::{ServeDir, ServeFile};
use tower_http::cors::{CorsLayer, AllowOrigin};
use axum::extract::DefaultBodyLimit;
use axum::Router;
use axum::http::{HeaderValue, Method};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .compact()
                .with_target(false)
        )
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gallerynet=info,warn".parse().unwrap()),
        )
        .init();

    // Configuration
    let mut db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "gallery.db".to_string());
    let model_path = std::env::var("MODEL_PATH").unwrap_or_else(|_| "assets/models/mobilenetv3.onnx".to_string());
    let upload_dir = PathBuf::from(std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string()));
    let thumbnail_dir = PathBuf::from(std::env::var("THUMBNAIL_DIR").unwrap_or_else(|_| "thumbnails".to_string()));
    let port = 3000;

    // Command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("GalleryNet - AI-powered media gallery");
        println!("Usage: gallerynet [OPTIONS]");
        println!("");
        println!("Options:");
        println!("  --db <path>       Path to the SQLite database (default: gallery.db)");
        println!("  --reset-faces     Clear all face detections and trigger re-scan");
        println!("  --help, -h        Show this help message");
        return Ok(());
    }

    // Parse --db parameter
    for i in 0..args.len() {
        if args[i] == "--db" && i + 1 < args.len() {
            db_path = args[i + 1].clone();
        }
    }

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

    // Initialize Infrastructure (Database first)
    println!("Initializing Database at {}...", db_path);
    let repo = Arc::new(SqliteRepository::new(&db_path)?);

    // Command line arguments - check THIS before loading heavy AI models
    if args.iter().any(|arg| arg == "--reset-faces") {
        println!("Resetting face index as requested...");
        match repo.reset_face_index() {
            Ok(_) => {
                println!("SUCCESS: Face index has been cleared and bounding boxes deleted.");
                println!("Restart the application normally to begin re-indexing.");
                return Ok(());
            }
            Err(e) => {
                eprintln!("ERROR: Failed to reset face index: {}", e);
                std::process::exit(1);
            }
        }
    }

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
        upload_dir.clone(),
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

    let group_faces_use_case = Arc::new(GroupFacesUseCase::new(
        repo.clone(),
    ));

    let tag_learning_use_case = Arc::new(TagLearningUseCase::new(
        repo.clone(),
    ));

    let fix_thumbnails_use_case = Arc::new(FixThumbnailsUseCase::new(
        repo.clone(),
        ai.clone(),
        hasher.clone(),
        upload_dir.clone(),
        thumbnail_dir.clone(),
    ));

    let external_search_use_case = Arc::new(ExternalSearchUseCase::new(
        repo.clone(),
        upload_dir.clone(),
    ));

    let index_faces_use_case = Arc::new(IndexFacesUseCase::new(
        repo.clone(),
        ai.clone(),
        upload_dir.clone(),
    ));

    let list_people_use_case = Arc::new(ListPeopleUseCase::new(
        repo.clone(),
    ));

    let find_similar_faces_use_case = Arc::new(FindSimilarFacesUseCase::new(
        repo.clone(),
    ));

    let (tx, _) = tokio::sync::broadcast::channel(100);

    // Initialize Background Tasks
    let task_runner = application::TaskRunner::new(
        fix_thumbnails_use_case.clone(),
        index_faces_use_case.clone(),
        group_faces_use_case.clone(),
        search_use_case.clone(),
        repo.clone(),
        tx.clone(),
    );

    let face_indexer_wakeup = task_runner.get_wakeup_notify();
    task_runner.start();

    // Initialize App State
    let state = AppState {
        upload_use_case,
        search_use_case,
        list_use_case,
        delete_use_case,
        group_use_case,
        group_faces_use_case,
        find_similar_faces_use_case,
        list_people_use_case,
        tag_learning_use_case,
        fix_thumbnails_use_case,

        external_search_use_case,
        repo: repo.clone(),
        upload_dir: upload_dir.clone(),
        auth_config: auth_config.clone(),
        upload_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
        login_rate_limiter: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        download_plans: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        face_indexer_wakeup,
        tx,
    };




    // --- ROUTING ARCHITECTURE ---

    // 1. Group all backend logic (your existing routes) and attach the body limit
    let api_routes = app_router(state)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024 * 1024)); // 10GB

    // 2. Configure the React SPA fallback (always accessible — it serves the login page too)
    let serve_react_app = ServeDir::new("frontend/dist")
        .not_found_service(ServeFile::new("frontend/dist/index.html"));

    // 3. Static file routes for media — must be auth-protected with security headers
    let static_uploads = Router::new()
        .nest_service("/uploads", ServeDir::new(upload_dir))
        .nest_service("/thumbnails", ServeDir::new(thumbnail_dir))
        .layer(axum::middleware::map_response(|mut response: axum::response::Response| async move {
            let headers = response.headers_mut();
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            headers.insert(
                axum::http::header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment"),
            );
            headers.insert(
                axum::http::header::HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
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

    // 4. Configure CORS — restrictive by default, configurable via CORS_ORIGIN env var
    let cors_layer = if let Ok(origin) = std::env::var("CORS_ORIGIN") {
        let origin = origin.trim().to_string();
        if let Ok(header_value) = HeaderValue::from_str(&origin) {
            CorsLayer::new()
                .allow_origin(AllowOrigin::exact(header_value))
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::COOKIE])
                .allow_credentials(true)
        } else {
            eprintln!("Warning: Invalid CORS_ORIGIN value '{}', denying cross-origin", origin);
            CorsLayer::new()
                .allow_origin(AllowOrigin::exact(HeaderValue::from_static("https://localhost")))
        }
    } else {
        // No CORS_ORIGIN set — deny cross-origin (same-origin requests don't need CORS)
        CorsLayer::new()
    };

    // 5. Assemble the final application router
    let app = Router::new()
        .nest("/api", api_routes) // All your endpoints now live under /api
        .merge(static_uploads)     // Auth-protected static file serving
        .layer(cors_layer)
        .fallback_service(serve_react_app); // Anything else returns the React App (login page)

    // ------------------------------------

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    println!("Server running on http://0.0.0.0:{}", port);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}
