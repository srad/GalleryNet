# GalleryNet

Image/video gallery with AI-powered visual similarity search. Rust backend + React frontend.

## Architecture

Hexagonal architecture with four layers:

```
src/
├── domain/          # Models, trait ports, error types (no dependencies)
│   ├── models.rs    # MediaItem, MediaSummary, MediaCounts, Folder, TrainedTagModel
│   └── ports.rs     # MediaRepository, AiProcessor, HashGenerator traits + DomainError
├── application/     # Use cases (orchestration logic)
│   ├── upload.rs    # UploadMediaUseCase — phash check, EXIF, thumbnail, feature extraction, save
│   ├── search.rs    # SearchSimilarUseCase — extract query features, find similar via DB
│   ├── list.rs      # ListMediaUseCase — paginated media listing with optional media_type filter and sort
│   ├── delete.rs    # DeleteMediaUseCase — single and batch delete (DB + files)
│   ├── group.rs     # GroupMediaUseCase — Union-Find clustering with rayon-parallelized pairwise cosine comparison (capped at 10k items, 5M edges)
│   ├── tag_learning.rs # TagLearningUseCase — Linear SVM with class-weighted training (pos_neg_weights), Platt-calibrated probabilities, hard negative mining, outlier filtering
│   ├── maintenance.rs # FixThumbnailsUseCase — Scans for media with missing phash ('no_hash'), re-generates thumbnails, phash, features, and updates DB
│   └── tasks.rs       # TaskRunner — Background scheduler for internal maintenance and cleanup
├── infrastructure/  # Trait implementations (adapters)

│   ├── sqlite_repo/       # SQLite + sqlite-vec (connection pool, split into submodules)
│   │   ├── mod.rs         # Pool struct, schema init, trait impl delegation, tag helpers
│   │   ├── media.rs       # CRUD: save, find, delete, list, counts, favorites
│   │   ├── folders.rs     # Folder operations: create, list, delete, rename, reorder, media membership
│   │   ├── tags.rs        # Tag operations: CRUD, auto-tag management, tag model persistence (Platt coefficients)
│   │   └── embeddings.rs  # Vector embedding retrieval: all embeddings, random sampling, KNN nearest neighbors
│   ├── ort_processor.rs   # MobileNetV3 ONNX inference via ort crate (session pool)
│   └── phash_generator.rs # Perceptual hashing for duplicate detection
├── presentation/    # HTTP layer
│   ├── api.rs       # Axum handlers: upload, search, list, get, delete, batch-delete, fix-thumbnails, batch-download (streaming), stats, login/logout, folders. Rate limiting, filename sanitization, error message sanitization, part-based zip splitting.
│   └── auth.rs      # Authentication middleware, AuthConfig, HMAC token generation/verification, constant-time password comparison, session generation counter for invalidation
├── events.ts            # Application-wide selective media update bus (zero-latency local sync)
├── useWebSocket.ts      # WebSocket connection manager — heartbeat, reconnection jitter, lag recovery
├── types.ts             # Shared types: MediaItem, MediaFilter, UploadProgress, Folder

└── components/
    ├── Icons.tsx        # Inline SVG icons (PhotoIcon, UploadIcon, SearchIcon, TagIcon)
    ├── LoadingIndicator.tsx # Standardized spinner component (inline, centered, and overlay variants)
    ├── TagFilter.tsx    # Tag filter dropdown with search, multi-select checkboxes, badge count, item count per tag
    ├── TagInput.tsx     # Tag editor with autocomplete suggestions, create-new-tag, keyboard nav
    ├── LoginView.tsx    # Password login form — shown when unauthenticated
    ├── MediaCard.tsx    # Thumbnail card with lazy loading, video play badge, hover overlay, selection checkbox
    ├── MediaModal.tsx   # Full-size media viewer with detail panel, EXIF data, keyboard navigation
    ├── Sidebar.tsx      # Navigation (Link-based) + server stats panel (counts, disk space, version)
    ├── GalleryView.tsx  # Infinite-scroll grid with route-based filtering, marquee selection, batch ops, and library picker
    └── SearchView.tsx   # Reactive visual search with URL-synced similarity and reference selection
```

## Key Technical Details

### Feature Extraction Pipeline
- Model: MobileNetV3-Large (pretrained ImageNet, classifier head removed)
- Input: 224x224 RGB, ImageNet-normalized (mean=[0.485,0.456,0.406], std=[0.229,0.224,0.225])
- Output: 1280-dimensional feature vector
- Preprocessing in `ort_processor.rs`: center crop to square → resize 224x224 → normalize → CHW tensor
- ONNX model lives at `assets/models/mobilenetv3.onnx`, exported via `scripts/mobilenetv3_export.py`

### Vector Storage & Similarity Search
- sqlite-vec virtual table with **cosine distance** metric (`distance_metric=cosine`)
- Cosine distance range: 0 (identical) to 2 (opposite)
- API similarity slider (0-100%) maps to max_distance: `2.0 * (1.0 - similarity/100.0)`
- Vectors stored as raw little-endian f32 bytes via unsafe reinterpret cast
- Media without valid feature vectors (failed extractions, ffmpeg unavailable) have no vec_media row and are excluded from similarity search

### Upload Flow — Images
1. Load image, parse EXIF, apply orientation correction
2. Extract `original_date` from EXIF (`DateTimeOriginal` → `DateTimeDigitized` → `DateTime`), fallback to filename pattern (e.g. `IMG_20240115_134530.jpg`), then `Utc::now()`
3. Generate perceptual hash from **oriented** image (DoubleGradient algorithm) → check for duplicates
4. Generate 224x224 JPEG thumbnail (resize_to_fill)
5. Extract 1280-d features from **original image bytes** (not thumbnail — ort_processor does its own preprocessing)
6. Save original + thumbnail to disk (sharded: `uploads/{id[0:2]}/{id[2:4]}/{uuid}.{ext}`)
7. Store metadata (with `media_type = 'image'`, `original_date`) in `media` table, embedding in `vec_media` (if features exist)

### Upload Flow — Videos
1. Extract up to 5 representative frames via **ffmpeg** (`thumbnail=300` filter — picks the most visually distinct frame from every ~300 frames, avoiding dark/blank frames)
2. Generate perceptual hash from **all frames** (hashes joined with `|` separator) → check for duplicates
3. Generate thumbnail + extract features from the **first representative frame**
4. Save original + thumbnail to disk, store metadata (with `media_type = 'video'`, `original_date`) + embedding

### Upload Handler — Multi-file & Streaming
- Frontend sends each file as a separate `POST /api/upload` request (one file per HTTP call)
- Handler streams multipart field data to a temp file chunk-by-chunk (never holds full file in RAM during receive)
- When multiple files are sent in a single multipart body, they are processed concurrently via `tokio::spawn`
- Duplicate detection returns HTTP 409; frontend marks as "skipped" without interrupting the upload queue
- Frontend concurrency limit: 3 simultaneous uploads (`MAX_CONCURRENT`), remaining files queue automatically

### Thumbnail Naming Convention
- Thumbnails are always JPEG regardless of original format
- Stored as `thumbnails/{id[0:2]}/{id[2:4]}/{uuid}.jpg`
- The `filename` field in the database stores the original extension (e.g., `ab/cd/uuid.mp4`)
- Frontend derives thumbnail URL by replacing the file extension with `.jpg`

### Database Schema
```sql
-- Metadata
CREATE TABLE media (
    id BLOB PRIMARY KEY,        -- UUID bytes
    filename TEXT NOT NULL,      -- sharded relative path
    original_filename TEXT NOT NULL,
    media_type TEXT NOT NULL DEFAULT 'image',  -- 'image' or 'video'
    phash TEXT NOT NULL,
    uploaded_at TEXT NOT NULL,   -- RFC 3339
    original_date TEXT NOT NULL, -- RFC 3339, from EXIF or filename or upload time
    width INTEGER,
    height INTEGER,
    size_bytes INTEGER NOT NULL,
    exif_json TEXT
);

-- Vector index (cosine distance)
CREATE VIRTUAL TABLE vec_media USING vec0(
    embedding float[1280] distance_metric=cosine
);
-- Linked to media via rowid

-- Virtual folders
CREATE TABLE folders (
    id BLOB PRIMARY KEY,        -- UUID bytes
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,    -- RFC 3339
    sort_order INTEGER NOT NULL DEFAULT 0
);

-- Folder-media junction (many-to-many)
CREATE TABLE folder_media (
    folder_id BLOB NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    media_id BLOB NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    PRIMARY KEY (folder_id, media_id)
);

-- Favorites
CREATE TABLE favorites (
    media_id BLOB PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);

-- Tags
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

-- Tag-media junction (many-to-many)
CREATE TABLE media_tags (
    media_id BLOB NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    is_auto INTEGER NOT NULL DEFAULT 0,   -- 0=manual, 1=auto-tagged by SVM
    confidence REAL,                       -- Platt-calibrated probability (auto-tags only)
    PRIMARY KEY (media_id, tag_id)
);
CREATE INDEX idx_media_tags_tag_id ON media_tags(tag_id);

-- Trained SVM tag models
CREATE TABLE tag_models (
    tag_id INTEGER PRIMARY KEY REFERENCES tags(id) ON DELETE CASCADE,
    weights BLOB NOT NULL,                 -- f64 weight vector (dim * 8 bytes)
    bias REAL NOT NULL,                    -- SVM decision boundary offset (rho)
    platt_a REAL NOT NULL DEFAULT -2.0,    -- Platt scaling coefficient A
    platt_b REAL NOT NULL DEFAULT 0.0,     -- Platt scaling coefficient B
    trained_at_count INTEGER NOT NULL DEFAULT 0,  -- manual positive count at training time
    version INTEGER NOT NULL DEFAULT 1
);
```

Auto-migration: on startup, if the `media_type` column is missing (pre-existing DB), the app runs `ALTER TABLE` to add it and backfills existing rows by checking file extension.

### Concurrency
- **SQLite**: Condvar-based connection pool (4 connections), WAL mode enabled, 5s busy timeout
- **ONNX Runtime**: Condvar-based session pool (10 sessions). Image preprocessing (load, crop, resize, normalize) happens *before* acquiring a session to minimize lock hold time. `Session::run()` requires `&mut self` in ort 2.0.0-rc.11, so pooling is necessary for concurrent inference.
- **WebSocket Broadcast**: Serialize-once, zero-cloning architecture. Messages are serialized to JSON once and shared via `Arc<str>` across all connected clients to minimize CPU and memory overhead.

## Build & Run


### Runtime Dependencies
- **ffmpeg** must be on PATH for video frame extraction (phash, thumbnails, features)

```bash
# Backend
cargo run
# Environment variables (all optional, shown with defaults):
#   DATABASE_PATH=gallery.db
#   MODEL_PATH=assets/models/mobilenetv3.onnx
#   UPLOAD_DIR=uploads
#   THUMBNAIL_DIR=thumbnails
#   GALLERY_PASSWORD=         # Set to enable password authentication (empty = no auth)
#   CORS_ORIGIN=              # Set to allow cross-origin requests from a specific origin (e.g. https://example.com). Unset = deny cross-origin.

# Frontend
cd frontend && npm install && npm run build
# Dev: npm run dev (Vite dev server on :5173, proxies /api to :3000)

# Docker
docker build -t gallerynet . && docker run -p 3000:3000 gallerynet
```

Server runs on port 3000. Serves the React SPA from `frontend/dist/` as fallback.

## API Endpoints

- `POST /api/login` — JSON body `{"password":"..."}`. Returns 200 with `Set-Cookie: gallery_session=<token>` (HttpOnly, Secure, SameSite=Strict) on success, 401 on wrong password, 429 after 10 attempts per 5-minute window per IP. Constant-time password comparison. No-op (always 200) if auth not configured.
- `POST /api/logout` — Clears the session cookie and invalidates all existing sessions (bumps generation counter). Always returns 200.
- `GET /api/ws` — WebSocket endpoint for real-time library updates. Requires authentication. Handles heartbeats (30s), lag detection, and selective metadata pushing (stripped of EXIF).
- `GET /api/auth-check` — Returns `{"authenticated": true/false, "required": true/false}`. Used by frontend on load to decide whether to show login screen.

- `POST /api/upload` — Multipart file upload (single or multiple files, max 1000 per request). Only allowed extensions (jpg, jpeg, png, gif, webp, bmp, tiff, tif, heic, heif, avif, mp4, mov, avi, mkv, webm). Single file returns `MediaItem` JSON. Multiple files returns array of `{media, error, filename}` results. Streams to temp file to avoid memory buffering. Image decode limits (10k×10k px, 400MB alloc) prevent pixel bombs. HTTP 409 for duplicates.
- `POST /api/search` — Multipart with `file` (image) and `similarity` (0-100). Returns array of `MediaItem`.
- `POST /api/media/group` — Group media by similarity. JSON body `{"folder_id": "...", "similarity": 80}`. Returns array of `MediaGroup` (groups of items).
- `GET /api/tags` — List all unique tags.
- `GET /api/tags/count?folder_id=...` — Count auto-tags in current view.
- `POST /api/tags/learn` — Learn from manual tags. JSON body `{"tag_name":"..."}`. Trains a linear SVM on all manual positives for the tag, computes Platt calibration, and auto-tags the entire library.
- `POST /api/tags/{id}/apply` — Apply learned tag model. Body `{"folder_id": "..."}`.
- `GET /api/media?page=1&limit=20&media_type=image&sort=desc&sort_by=date&tags=a,b` — Paginated media list. Optional `media_type` filter (`image` or `video`). Optional `sort` param (`asc` or `desc`, default `desc`). Optional `sort_by` param (`date` or `size`, default `date`). Optional `favorite=true`. Optional `tags` (comma-separated, AND filter). Returns array of `MediaSummary`.
- `GET /api/media/{id}` — Get full `MediaItem` by UUID, including `exif_json`. Returns 404 if not found.
- `POST /api/media/{id}/favorite` — Toggle favorite status. JSON body `{"favorite": true/false}`. Returns 200.
- `GET /api/stats` — Server statistics: `{version, total_files, total_images, total_videos, total_size_bytes, disk_free_bytes, disk_total_bytes}`.
- `DELETE /api/media/{id}` — Delete single media item. Returns 204 or 404.
- `POST /api/media/batch-delete` — JSON body `["uuid1", "uuid2", ...]`. Returns `{"deleted": N}`. Deletes DB rows, embeddings, originals, and thumbnails.
- `POST /api/media/fix-thumbnails` — Triggers background repair of media items with missing perceptual hashes (re-generates thumbnails, features, and metadata).
- `POST /api/media/download/plan` — JSON body `["uuid1", "uuid2", ...]`. Returns `DownloadPlan` with part IDs and size estimates. Partitions large requests into parts under 2GB.
- `GET /api/media/download/stream/{part_id}` — Incremental zip stream for a specific plan part. Uses `async_zip` for on-the-fly streaming with `Compression::Stored`.
- `POST /api/media/download` — Legacy/simple batch download. Returns single zip stream if under 2GB.

- `GET /api/folders` — List all folders with item counts, ordered by `sort_order`. Returns array of `{id, name, created_at, item_count, sort_order}`.
- `POST /api/folders` — Create folder. JSON body `{"name":"..."}`. Returns created folder. New folders get `sort_order = max + 1`.
- `PUT /api/folders/reorder` — Reorder folders. JSON body `["folder_uuid1", "folder_uuid2", ...]` (ordered array of folder IDs). Sets `sort_order` to array index. Returns 204.
- `DELETE /api/folders/{id}` — Delete folder (does NOT delete media files). Returns 204.
- `GET /api/folders/{id}/media?page=&limit=&media_type=&sort=&sort_by=` — Paginated media list within a folder. Same params as `/api/media`.
- `POST /api/folders/{id}/media` — Add media to folder. JSON body `["uuid1","uuid2"]`. Uses `INSERT OR IGNORE` for idempotency.
- `POST /api/folders/{id}/media/remove` — Remove media from folder. JSON body `{"media_ids":["uuid1"]}`.
- `GET /api/folders/{id}/download` — Returns `DownloadPlan` for all media in folder. Large folders auto-split into multiple zip parts.

- `/uploads/*` and `/thumbnails/*` — Static file serving with `Content-Disposition: attachment` and `X-Content-Type-Options: nosniff` headers.

## Frontend Architecture

- **Responsive layout**: Fully responsive across smartphones, tablets, and desktops. Sidebar is an off-canvas drawer on mobile (`<md`) triggered by a hamburger menu in a fixed top header bar; on desktop (`md:`) it's a static sidebar. Toolbar controls wrap on small screens with icon-only buttons at `<sm` and text labels at `sm:`. Grid columns scale from 2 (mobile) to 8 (xl). `MediaModal` uses reduced padding on mobile with touch swipe navigation. Selection toolbar is full-width on mobile, centered pill on desktop. All padding values are tiered (`p-4`/`p-8`) using Tailwind responsive prefixes.
- **Auth gate**: `App` checks `/api/auth-check` and fetches folders during a global `init` sequence. UI rendering is blocked until authentication status is known and initial metadata is available, preventing layout flickering. All API calls use `apiFetch()` from `auth.ts` which fires a `gallerynet-unauthorized` custom window event on 401 responses, causing `App` to switch back to the login screen.
- **Route-based Navigation**: Uses `react-router-dom` for all views. Main gallery at `/`, favorites at `/favorites`, search at `/search`, and folders at `/folders/:id`. Browser back/forward buttons and bookmarks are fully supported.
- **Persistent views**: Main tab views (`GalleryView`, `SearchView`) are kept mounted via `className="hidden"` logic driven by the current route. This ensures that long-running background tasks like **active uploads** persist even when navigating between different sections of the app.
- **Media detail modal**: `MediaModal` is triggered by the `?media={id}` query parameter. This allows direct deep-linking to specific images. If the linked item isn't in the current grid, it is fetched independently via `GET /api/media/{id}`. Supports keyboard navigation, touch swipes, and browser history (closing the modal goes "Back").
- **Reactive Visual Search**: The search view is fully automated. Results refresh instantly whenever the reference image changes (via file upload or "Select from Library") or the similarity slider is adjusted. Search parameters (source ID and similarity threshold) are synced to the URL.
- **Infinite scroll**: `GalleryView` uses `IntersectionObserver` with `root: null` (viewport) and 400px `rootMargin` to pre-fetch the next page. Pages are 60 items. Race condition protection via fetch ID counter and refs for mutable pagination state.
- **Lazy loading**: Thumbnail `<img>` tags use `loading="lazy"` and `decoding="async"`.
- **Media type filter**: Segmented button (All / Photos / Videos) triggers route-based re-fetch with `media_type` query param. Resets pagination on change. Persisted to `localStorage`.
- **Sort order**: Dropdown menu with four options (Newest, Oldest, Largest, Smallest) sends `sort=asc|desc` and `sort_by=date|size` to API. Both field and direction are persisted to `localStorage`.
- **Group by Similarity**: Toggle button switches view to grouped mode. Fetches clusters from `/api/media/group`. Includes a similarity slider (50-99%) that fires on pointer release. Shows a standardized `LoadingIndicator` overlay while processing.
- **Standardized Loading**: Global `LoadingIndicator.tsx` provides consistent visual feedback for initial app load, pagination, similarity search, and batch actions.
- **Virtual folders**: Organizational folders (many-to-many relationship with media). Sidebar shows folder list with Link-based navigation. Features an "Add from Library" button that opens a `LibraryPicker` modal. `LibraryPicker` supports a `singleSelect` mode specifically for choosing search references. Folders are drag-to-reorder via native HTML5 drag-and-drop. Supports **drag-and-drop of media items** directly from the gallery or search results into folders, with a success checkmark confirmation.
- **Keyboard Shortcuts**: Native-like gallery experience with `Ctrl+A` (or `Cmd+A`) to select all loaded items, and `Delete` (or `Backspace`) to trigger batch deletion or removal from the current folder. Shortcuts are automatically disabled when typing in inputs.
- **Global Drag-and-Drop**: Supports dragging files anywhere into the browser window. A global overlay provides visual feedback. Files are context-aware: dropping into a virtual folder adds them to that folder; dropping elsewhere adds them to the main library. Implemented via `App` level event listeners delegating to `GalleryView` refs.


## Dependencies

Key crates: `axum` (HTTP), `ort` (ONNX Runtime), `rusqlite` + `sqlite-vec` (DB + vectors), `image` (processing), `image_hasher` (phash), `ndarray` (tensors), `linfa` + `linfa-svm` (SVM training & Platt scaling), `kamadak-exif` (EXIF parsing), `libc` (disk space on Linux), `zip` (simple zip), `async_zip` (streaming zip), `tokio-util` (I/O compat), `mime_guess` (content-type detection for downloads).


External: `ffmpeg` (video frame extraction).

## Important Conventions
- Domain layer has zero external dependencies — all I/O goes through trait ports
- Concurrency via Condvar-based pools (no external pool crate needed)
- UUIDs stored as 16-byte BLOBs in SQLite, not strings
- Thumbnails are always JPEG regardless of original format, named `{uuid}.jpg` (not the original extension)
- Phash is orientation-corrected for images; multi-frame (joined with `|`) for videos
- `media_type` column stores `'image'` or `'video'` — set during upload based on file extension, used for filtering
- `original_date` extracted from EXIF (DateTimeOriginal → DateTimeDigitized → DateTime), filename pattern matching (`YYYYMMDD` or `YYYY-MM-DD`), then fallback to `Utc::now()`. Gallery sorts by this field.
- EXIF datetime format from `kamadak-exif` `display_value()` is `"YYYY:MM:DD HH:MM:SS"` (colon-separated date)
- `SUM(CASE ...)` in SQLite returns `NULL` on empty tables — wrapped in `COALESCE(..., 0)` in `media_counts` query
- `filename` is used as a stable identifier (React key, modal selection tracking) to avoid index-shift bugs during uploads
- `MediaSummary` (list endpoint) doesn't include `exif_json` — the separate `GET /api/media/{id}` endpoint returns full `MediaItem` with EXIF for the modal
- **Real-time Synchronization**: WebSocket updates utilize a serialize-once, zero-cloning broadcast architecture. `MediaUpdated` events include full metadata but strip `exif_json` to minimize bandwidth while allowing instant UI rendering of newly favorited or discovered items.
- **Sorting Resilience**: The gallery uses a centralized sorting engine that re-evaluates item positions whenever new media is created or metadata (date/size) changes, ensuring correct order without reloads.
- **Self-Healing**: The system utilizes an internal `TaskRunner` to automatically trigger repair tasks 15 seconds after startup and every 24 hours thereafter. It re-generates thumbnails and metadata for any items that failed processing (identified by `phash='no_hash'`).

- Disk space detection is platform-conditional: `GetDiskFreeSpaceExW` on Windows, `libc::statvfs` on Linux/macOS

- App version is read from `Cargo.toml` at compile time via `env!("CARGO_PKG_VERSION")`
- The ONNX model must be re-exported if changing the feature extraction architecture (see `scripts/mobilenetv3_export.py`, requires Python with `torch`, `timm`, `onnx`)
- **Dialogs**: Always use `ConfirmDialog.tsx` for confirmations (delete, destructive actions) and `AlertDialog.tsx` for alerts/notifications. Avoid using the native `window.alert` or `window.confirm`.
