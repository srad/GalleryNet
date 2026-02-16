# GalleryNet

Image/video gallery with AI-powered visual similarity search. Rust backend + React frontend.

## Architecture

Hexagonal architecture with four layers:

```
src/
├── domain/          # Models, trait ports, error types (no dependencies)
│   ├── models.rs    # MediaItem, MediaSummary, MediaCounts, Folder
│   └── ports.rs     # MediaRepository, AiProcessor, HashGenerator traits + DomainError
├── application/     # Use cases (orchestration logic)
│   ├── upload.rs    # UploadMediaUseCase — phash check, EXIF, thumbnail, feature extraction, save
│   ├── search.rs    # SearchSimilarUseCase — extract query features, find similar via DB
│   ├── list.rs      # ListMediaUseCase — paginated media listing with optional media_type filter and sort
│   ├── delete.rs    # DeleteMediaUseCase — single and batch delete (DB + files)
│   ├── group.rs     # GroupMediaUseCase — Union-Find clustering with rayon-parallelized pairwise cosine comparison
│   └── tag_learning.rs # TagLearningUseCase — SVM-based iterative learning and auto-tagging
├── infrastructure/  # Trait implementations (adapters)
│   ├── sqlite_repo/       # SQLite + sqlite-vec (connection pool, split into submodules)
│   │   ├── mod.rs         # Pool struct, schema init, trait impl delegation, tag helpers
│   │   ├── media.rs       # CRUD: save, find, delete, list, counts, favorites
│   │   ├── folders.rs     # Folder operations: create, list, delete, rename, reorder, media membership
│   │   ├── tags.rs        # Tag operations: get_all, update single, update batch
│   │   └── embeddings.rs  # Vector embedding retrieval for similarity grouping
│   ├── ort_processor.rs   # MobileNetV3 ONNX inference via ort crate (session pool)
│   └── phash_generator.rs # Perceptual hashing for duplicate detection
├── presentation/    # HTTP layer
│   ├── api.rs       # Axum handlers: upload, search, list, get, delete, batch-delete, batch-download, stats, login/logout, folders
│   └── auth.rs      # Authentication middleware, AuthConfig, HMAC token generation/verification
└── main.rs          # Wiring, config, server startup

frontend/src/
├── App.tsx              # Root component — tab routing, filter state, refresh key, upload progress, beforeunload guard, auth gate, folder navigation, mobile sidebar toggle + header bar
├── auth.ts              # Auth utilities: apiFetch (401-intercepting fetch wrapper), fireUnauthorized event dispatcher
├── types.ts             # Shared types: MediaItem, MediaFilter, UploadProgress, Folder
└── components/
    ├── Icons.tsx        # Inline SVG icons (PhotoIcon, UploadIcon, SearchIcon, TagIcon)
    ├── TagFilter.tsx    # Tag filter dropdown with search, multi-select checkboxes, badge count
    ├── TagInput.tsx     # Tag editor with autocomplete suggestions, create-new-tag, keyboard nav
    ├── LoginView.tsx    # Password login form — shown when unauthenticated
    ├── MediaCard.tsx    # Thumbnail card with lazy loading, video play badge, hover overlay, selection checkbox
    ├── MediaModal.tsx   # Full-size media viewer with detail panel, EXIF data, keyboard navigation
    ├── Sidebar.tsx      # Navigation + server stats panel (counts, disk space, version)
    ├── GalleryView.tsx  # Infinite-scroll grid with sort toggle, multi-select, batch ops, inline upload, library picker
    └── SearchView.tsx   # Visual similarity search with reference image + similarity slider
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
    PRIMARY KEY (media_id, tag_id)
);
CREATE INDEX idx_media_tags_tag_id ON media_tags(tag_id);
```

Auto-migration: on startup, if the `media_type` column is missing (pre-existing DB), the app runs `ALTER TABLE` to add it and backfills existing rows by checking file extension.

### Concurrency
- **SQLite**: Condvar-based connection pool (4 connections), WAL mode enabled, 5s busy timeout
- **ONNX Runtime**: Condvar-based session pool (10 sessions). Image preprocessing (load, crop, resize, normalize) happens *before* acquiring a session to minimize lock hold time. `Session::run()` requires `&mut self` in ort 2.0.0-rc.11, so pooling is necessary for concurrent inference.

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

# Frontend
cd frontend && npm install && npm run build
# Dev: npm run dev (Vite dev server on :5173, proxies /api to :3000)

# Docker
docker build -t gallerynet . && docker run -p 3000:3000 gallerynet
```

Server runs on port 3000. Serves the React SPA from `frontend/dist/` as fallback.

## API Endpoints

- `POST /api/login` — JSON body `{"password":"..."}`. Returns 200 with `Set-Cookie: gallery_session=<token>` on success, 401 on wrong password. No-op (always 200) if auth not configured.
- `POST /api/logout` — Clears the session cookie. Always returns 200.
- `GET /api/auth-check` — Returns `{"authenticated": true/false, "required": true/false}`. Used by frontend on load to decide whether to show login screen.
- `POST /api/upload` — Multipart file upload (single or multiple files). Single file returns `MediaItem` JSON. Multiple files returns array of `{media, error, filename}` results. Streams to temp file to avoid memory buffering. HTTP 409 for duplicates.
- `POST /api/search` — Multipart with `file` (image) and `similarity` (0-100). Returns array of `MediaItem`.
- `POST /api/media/group` — Group media by similarity. JSON body `{"folder_id": "...", "similarity": 80}`. Returns array of `MediaGroup` (groups of items).
- `GET /api/tags` — List all unique tags.
- `GET /api/tags/count?folder_id=...` — Count auto-tags in current view.
- `POST /api/tags/learn` — Learn from manual tags. JSON body `{"tag_name":"...", "positive_ids": [...]}`.
- `POST /api/tags/{id}/apply` — Apply learned tag model. Body `{"folder_id": "..."}`.
- `GET /api/media?page=1&limit=20&media_type=image&sort=desc&tags=a,b` — Paginated media list. Optional `media_type` filter (`image` or `video`). Optional `sort` param (`asc` or `desc`, default `desc`). Optional `favorite=true`. Optional `tags` (comma-separated, AND filter). Sorted by `original_date`. Returns array of `MediaSummary`.
- `GET /api/media/{id}` — Get full `MediaItem` by UUID, including `exif_json`. Returns 404 if not found.
- `POST /api/media/{id}/favorite` — Toggle favorite status. JSON body `{"favorite": true/false}`. Returns 200.
- `GET /api/stats` — Server statistics: `{version, total_files, total_images, total_videos, total_size_bytes, disk_free_bytes, disk_total_bytes}`.
- `DELETE /api/media/{id}` — Delete single media item. Returns 204 or 404.
- `POST /api/media/batch-delete` — JSON body `["uuid1", "uuid2", ...]`. Returns `{"deleted": N}`. Deletes DB rows, embeddings, originals, and thumbnails.
- `POST /api/media/download` — JSON body `["uuid1", "uuid2", ...]`. Single file returns the file directly with appropriate Content-Type. Multiple files returns a zip archive (`application/zip`). Filenames in zip are deduplicated with `_N` suffix.
- `GET /api/folders` — List all folders with item counts, ordered by `sort_order`. Returns array of `{id, name, created_at, item_count, sort_order}`.
- `POST /api/folders` — Create folder. JSON body `{"name":"..."}`. Returns created folder. New folders get `sort_order = max + 1`.
- `PUT /api/folders/reorder` — Reorder folders. JSON body `["folder_uuid1", "folder_uuid2", ...]` (ordered array of folder IDs). Sets `sort_order` to array index. Returns 204.
- `DELETE /api/folders/{id}` — Delete folder (does NOT delete media files). Returns 204.
- `GET /api/folders/{id}/media?page=&limit=&media_type=&sort=` — Paginated media list within a folder. Same params as `/api/media`.
- `POST /api/folders/{id}/media` — Add media to folder. JSON body `["uuid1","uuid2"]`. Uses `INSERT OR IGNORE` for idempotency.
- `POST /api/folders/{id}/media/remove` — Remove media from folder. JSON body `{"media_ids":["uuid1"]}`.
- `GET /api/folders/{id}/download` — Download all media in folder as zip.
- `/uploads/*` and `/thumbnails/*` — Static file serving.

## Frontend Architecture

- **Responsive layout**: Fully responsive across smartphones, tablets, and desktops. Sidebar is an off-canvas drawer on mobile (`<md`) triggered by a hamburger menu in a fixed top header bar; on desktop (`md:`) it's a static sidebar. Toolbar controls wrap on small screens with icon-only buttons at `<sm` and text labels at `sm:`. Grid columns scale from 2 (mobile) to 8 (xl). `MediaModal` uses reduced padding on mobile with touch swipe navigation. Selection toolbar is full-width on mobile, centered pill on desktop. All padding values are tiered (`p-4`/`p-8`) using Tailwind responsive prefixes.
- **Auth gate**: `App` checks `/api/auth-check` on mount. If auth is required and user is not authenticated, shows `LoginView` instead of the main app. All API calls use `apiFetch()` from `auth.ts` which fires a `gallerynet-unauthorized` custom window event on 401 responses, causing `App` to switch back to the login screen. Upload XHRs check for 401 explicitly and call `fireUnauthorized()`.
- **Infinite scroll**: `GalleryView` uses `IntersectionObserver` with `root: null` (viewport) and 400px `rootMargin` to pre-fetch the next page. Pages are 60 items. Race condition protection via fetch ID counter and refs for mutable pagination state.
- **Lazy loading**: Thumbnail `<img>` tags use `loading="lazy"` and `decoding="async"`.
- **Media type filter**: Segmented button (All / Photos / Videos) triggers API re-fetch with `media_type` query param. Resets pagination on change. Persisted to `localStorage`.
- **Sort order**: Toggle button (Newest / Oldest) sends `sort=asc|desc` to API. Sorted by `original_date`. Persisted to `localStorage`.
- **Group by Similarity**: Toggle button switches view to grouped mode. Fetches clusters from `/api/media/group`. Includes a similarity slider (50-99%) that fires on pointer release (not during drag). Shows a "Computing similarity groups..." overlay while the server processes. All toolbar controls and sidebar navigation are disabled during computation. Renders grid as sections with headers (Group 1, Group 2...).
- **Gentle merge on upload**: When `refreshKey` changes after upload, new items are merged into the existing grid without resetting scroll position. Full reset only on filter/sort changes.
- **Persistent views**: All tab views (`GalleryView`, `SearchView`) are always mounted with `className="hidden"` toggling, so upload state (queue, XHRs, progress) persists across tab switches.
- **Upload queue**: Managed locally within `GalleryView` (inline). Files upload individually (one per HTTP request) with concurrency limit of 3. Per-file states: pending → uploading → done / duplicate / error. Duplicates (HTTP 409) show amber "Skipped" indicator.
- **Upload progress**: Inline within `GalleryView` (top of grid). Shows counts (done/skipped/failed) and a progress bar. No longer global/sidebar-based.
- **Beforeunload guard**: `GalleryView` registers `beforeunload` listener during group computation to prevent accidental page closure.
- **Stats sidebar**: Fetches `/api/stats` on load and after each upload. Shows file counts, storage used, disk free space with color-coded bar, and app version.
- **Video indicators**: Cards detect video by file extension and show a translucent play button overlay.
- **Favorites**: Dedicated sidebar tab showing only favorite items. Toggle button on media cards (heart icon) and in the detail modal.
- **Refresh key pattern**: `App` holds a `refreshKey` counter incremented after uploads. Both `GalleryView` and `Sidebar` re-fetch when it changes.
- **Media detail modal**: `MediaModal` shows full-size image/video with a detail panel. Fetches full `MediaItem` via `GET /api/media/{id}` to display EXIF data (collapsible table). Keyboard navigation (Arrow Left/Right, Escape), prev/next buttons, backdrop click to close, touch swipe left/right for mobile navigation. Selected item tracked by `filename` (stable across grid mutations).
- **Multi-select & batch operations**: `GalleryView` has a "Select" toggle button that enters selection mode. In selection mode, clicking a card toggles its selection (blue checkbox overlay on `MediaCard`). Shift-click selects a range. Floating toolbar appears at the bottom with: count display, select all/deselect all, download (single file or zip with streamed progress overlay), delete (with confirmation dialog), and cancel. Escape key exits selection mode. Selection mode auto-closes after successful download or delete. Selection state is cleared on filter/sort changes.
- **Marquee (rubber-band) selection**: Click and drag on the grid background to draw a selection rectangle (like Windows Explorer). Cards intersecting the rectangle are live-selected as you drag. Automatically enters selection mode when a drag starts. Hold Ctrl/Cmd while dragging to add to existing selection. Uses absolute page coordinates with scroll offset for accurate hit-testing. 5px deadzone prevents accidental marquee on normal clicks. Each `MediaCard` has a `data-filename` attribute for DOM-based intersection detection.
- **Download progress**: Batch download streams the response via `ReadableStream` and shows a modal overlay with spinner, progress bar (when `Content-Length` is available), and received/total byte counts. Zip filename format: `gallerynet_<count>_<YYYYMMDD_HHMMSS>.zip`.
- **Virtual folders**: Organizational folders (many-to-many relationship with media). Sidebar shows folder list with item counts, inline "New folder" input, and delete (X) button per folder. Clicking a folder opens a folder-specific `GalleryView` (fetches from `/api/folders/{id}/media`) with a back button, folder name header, and inline upload button. Also features an "Add from Library" button that opens a `LibraryPicker` modal (reusing `GalleryView` logic) to add existing media. Selection toolbar includes "Add to folder" dropdown picker (available in both main gallery and folder views) and "Remove from folder" button (only in folder view). Deleting a folder removes only the folder and associations, not the actual media files. Folder list refreshes on upload completion and folder mutations. Folders are drag-to-reorder via native HTML5 drag-and-drop with a grip handle; order is persisted via `PUT /api/folders/reorder` and stored in the `sort_order` column.

## Dependencies

Key crates: `axum` (HTTP), `ort` (ONNX Runtime), `rusqlite` + `sqlite-vec` (DB + vectors), `image` (processing), `image_hasher` (phash), `ndarray` (tensors), `kamadak-exif` (EXIF parsing), `libc` (disk space on Linux), `zip` (batch download archive), `mime_guess` (content-type detection for downloads).

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
- Disk space detection is platform-conditional: `GetDiskFreeSpaceExW` on Windows, `libc::statvfs` on Linux/macOS
- App version is read from `Cargo.toml` at compile time via `env!("CARGO_PKG_VERSION")`
- The ONNX model must be re-exported if changing the feature extraction architecture (see `scripts/mobilenetv3_export.py`, requires Python with `torch`, `timm`, `onnx`)
