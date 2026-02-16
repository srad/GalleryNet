<p align="center">
  <img src="assets/logo.png" alt="GalleryNet Logo" width="120" />
</p>

<h1 align="center">GalleryNet</h1>

<p align="center">
  <strong>Self-hosted media gallery with AI-powered visual similarity search, grouping, tagging</strong>
</p>

<p align="center">
  <a href="https://teamcity.sedrad.com/viewType.html?buildTypeId=GalleryNet_Build&guest=1">
    <img src="https://teamcity.sedrad.com/app/rest/builds/buildType:(id:GalleryNet_Build)/statusIcon" alt="Build Status">
  </a>

  <img src="https://img.shields.io/badge/rust-2021-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust">

  <img src="https://img.shields.io/badge/react-19-61DAFB?style=flat-square&logo=react&logoColor=white" alt="React">

  <img src="https://img.shields.io/badge/ONNX_Runtime-MobileNetV3-green?style=flat-square" alt="ONNX Runtime">

  <a href="https://hub.docker.com/r/sedrad/gallerynet/tags">
    <img src="https://img.shields.io/docker/image-size/sedrad/gallerynet/v1?style=flat-square&logo=docker&logoColor=white&label=docker%20image%20size" alt="Docker Size">
  </a>

  <a href="https://github.com/srad/GalleryNet/stargazers">
    <img src="https://img.shields.io/github/stars/srad/GalleryNet?style=flat-square" alt="Stars">
  </a>
</p>

<p align="center">
  Upload photos and videos, organize them into folders, find visually similar images with AI &mdash; all running on your own hardware. No cloud, no API keys, fully private.
</p>

---

<p align="center">
  <img alt="GalleryNet Screenshot" width="900" alt="screenshot_1" src="https://github.com/user-attachments/assets/72613ee9-ab24-4d23-94f3-9b13a6fa4836" />
</p>

---

## Highlights

- **AI Visual Search** &mdash; Find similar images and videos using MobileNetV3 embeddings and cosine similarity via [sqlite-vec](https://github.com/asg017/sqlite-vec)
- **Duplicate Detection** &mdash; Perceptual hashing automatically prevents uploading the same image or video twice
- **Item Grouping** &mdash; Grouping of similar images and videos with gradual similarity slider
- **AI-Powered Tagging** &mdash; Tag 3+ examples and let a linear SVM with Platt-calibrated probabilities auto-tag your entire library.
- **Favorites** &mdash; Mark items as favorites to quickly access your best shots in a dedicated view
- **Video Support** &mdash; Full video upload with intelligent frame extraction via ffmpeg for thumbnails, hashing, and feature vectors
- **Virtual Folders** &mdash; Organize media into folders without moving files. Many-to-many: one photo can live in multiple folders. Supports **drag-and-drop** of media items directly into folders.
- **Deep Linking** &mdash; Direct URLs for folders, favorites, and individual media items. Modals and search states are bookmarkable.
- **Batch Operations** &mdash; Multi-select with marquee (rubber-band) selection, keyboard shortcuts (`Ctrl+A`, `Delete`), batch download as zip, batch delete, batch add-to-folder

- **EXIF Metadata** &mdash; View camera details, date taken, GPS, exposure, and more in the detail modal
- **Password Protection** &mdash; Optional authentication via a single environment variable
- **100% Self-Hosted** &mdash; Everything runs on your machine. No cloud. No telemetry. Your data stays yours.

## Features

### Gallery
- Fully responsive layout &mdash; works on smartphones, tablets, and desktops
- Route-based navigation &mdash; supports browser history, back/forward buttons, and deep links
- Infinite-scroll grid with lazy-loaded thumbnails
- **Keyboard Shortcuts** &mdash; Select all (`Ctrl+A`), delete (`Del`), navigation (`Arrows`, `Esc`)
- Filter by media type (All / Photos / Videos)

- Sort by date (Newest / Oldest) based on EXIF original date
- Full-size media viewer with EXIF detail panel, keyboard navigation, and touch swipe
- Deep-linked modals &mdash; share links directly to specific media items via `?media=<id>`

### Favorites
- Mark any photo or video as a favorite with a single click
- Dedicated "Favorites" tab in the sidebar
- Filter by favorites within virtual folders to see best-of collections

### AI-Powered Visual Search
- Upload a reference image and find visually similar photos in your library
- Adjustable similarity threshold (0&ndash;100%)
- **Group by Similarity** &mdash; Automatically cluster your entire gallery into visually similar groups with a single click
- **Active Learning Tags** &mdash; Tag 3+ items with the same name and click "Auto Tag" to train an AI model that automatically labels matching items in your gallery or folder.
- Powered by MobileNetV3-Large with 1280-dimensional feature vectors

- Vector similarity search using cosine distance in sqlite-vec

### Smart Upload
- Drag-and-drop or file picker with multi-file support
- Concurrent upload queue (3 simultaneous uploads)
- Per-file progress tracking with overall progress in the sidebar
- Automatic duplicate detection &mdash; duplicates are skipped, not rejected as errors
- EXIF date extraction with filename pattern fallback

### Virtual Folders
- Create folders to organize your media
- **Drag-and-drop** items directly into folders from the gallery, search results, or detail modal
- Add selected items to any folder from the gallery toolbar
- Each folder has its own gallery view with filtering and sorting
- Upload directly into a folder &mdash; media is added to the gallery and the folder
- Remove items from folders without deleting the actual files

### Multi-Select & Batch Operations
- Click "Select" or drag a marquee rectangle to select multiple items
- **Keyboard support** &mdash; `Ctrl+A` to select all, `Delete` to batch delete or remove from folder
- Shift-click for range selection, Ctrl/Cmd+drag for additive selection

- Batch download as a single file or zip archive with streaming progress
- Batch delete with confirmation
- Add selection to any folder via dropdown picker

### Video Handling
- Upload any video format supported by ffmpeg
- Intelligent frame extraction using ffmpeg's `thumbnail` filter (picks visually distinct frames)
- Thumbnail and AI features generated from representative frames
- Play badge overlay on video cards in the gallery

## Quick Start with Docker

The easiest way to run GalleryNet is with Docker:

```bash
docker run -d \
  --name gallerynet \
  -p 3000:3000 \
  -v gallerynet-data:/app/data \
  -e DATABASE_PATH=/app/data/gallery.db \
  -e UPLOAD_DIR=/app/data/uploads \
  -e THUMBNAIL_DIR=/app/data/thumbnails \
  sedrad/gallerynet
```

Then open **http://localhost:3000** in your browser.

### With Password Protection

```bash
docker run -d \
  --name gallerynet \
  -p 3000:3000 \
  -v gallerynet-data:/app/data \
  -e DATABASE_PATH=/app/data/gallery.db \
  -e UPLOAD_DIR=/app/data/uploads \
  -e THUMBNAIL_DIR=/app/data/thumbnails \
  -e GALLERY_PASSWORD=your-secret-password \
  sedrad/gallerynet
```

### Docker Compose

```yaml
services:
  gallerynet:
    image: sedrad/gallerynet
    container_name: gallerynet
    ports:
      - "3000:3000"
    environment:
      - DATABASE_PATH=/app/data/gallery.db
      - UPLOAD_DIR=/app/data/uploads
      - THUMBNAIL_DIR=/app/data/thumbnails
      # - GALLERY_PASSWORD=your-secret-password
    volumes:
      - ./data:/app/data
    restart: unless-stopped
```

```bash
docker compose up -d
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_PATH` | `gallery.db` | Path to the SQLite database file |
| `UPLOAD_DIR` | `uploads` | Directory for original uploaded files |
| `THUMBNAIL_DIR` | `thumbnails` | Directory for generated thumbnails |
| `MODEL_PATH` | `assets/models/mobilenetv3.onnx` | Path to the ONNX model file |
| `GALLERY_PASSWORD` | *(empty)* | Set to enable password authentication. Leave empty for no auth |

## Build from Source

### Prerequisites

- **Rust** &mdash; Latest stable toolchain
- **Node.js** &mdash; v18+
- **ffmpeg** &mdash; Must be on PATH for video support

### Build & Run

```bash
# Clone the repository
git clone https://github.com/srad/GalleryNet.git
cd GalleryNet

# Build the frontend
cd frontend && npm install && npm run build && cd ..

# Run the server
cargo run --release
```

The server starts on **http://localhost:3000**.

### Development

```bash
# Backend (auto-reload with cargo-watch)
cargo watch -x run

# Frontend (Vite dev server with HMR, proxies /api to :3000)
cd frontend && npm run dev
```

## Architecture

GalleryNet follows **Hexagonal Architecture** with clean separation of concerns:

```
src/
├── domain/          # Models & trait ports (zero dependencies)
├── application/     # Use cases (upload, search, list, delete)
├── infrastructure/  # SQLite, ONNX Runtime, perceptual hashing
├── presentation/    # Axum HTTP handlers & auth middleware
└── main.rs          # Wiring & server startup
```

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust, [Axum](https://github.com/tokio-rs/axum), Tokio |
| Database | SQLite + [sqlite-vec](https://github.com/asg017/sqlite-vec) |
| AI/ML | [ort](https://github.com/pykeio/ort) (ONNX Runtime), MobileNetV3-Large, [linfa-svm](https://crates.io/crates/linfa-svm) (tag learning) |
| Frontend | React 19, TypeScript, Tailwind CSS v4, Vite |
| Video | ffmpeg (frame extraction) |
| Hashing | [image_hasher](https://crates.io/crates/image_hasher) (perceptual hashing) |

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/upload` | Upload media (multipart). Returns `MediaItem`. 409 for duplicates |
| `POST` | `/api/search` | Visual similarity search. Multipart with `file` + `similarity` |
| `GET` | `/api/media` | Paginated media list. Params: `page`, `limit`, `media_type`, `sort` |
| `GET` | `/api/media/{id}` | Get single media item with EXIF data |
| `POST` | `/api/media/{id}/favorite` | Toggle favorite status. Body: `{"favorite": true/false}` |
| `DELETE` | `/api/media/{id}` | Delete single media item |
| `POST` | `/api/media/batch-delete` | Batch delete. Body: `["uuid1", ...]` |
| `POST` | `/api/media/download` | Batch download as zip. Body: `["uuid1", ...]` |
| `GET` | `/api/tags` | List all unique tags |
| `GET` | `/api/tags/count` | Count auto-tags in current view |
| `POST` | `/api/tags/learn` | Train model from manual tags. Body: `{"tag_name": "..."}` |
| `POST` | `/api/tags/auto-tag` | Apply all trained models to current scope |
| `GET` | `/api/folders` | List all folders with item counts |

| `POST` | `/api/folders` | Create folder. Body: `{"name": "..."}` |
| `DELETE` | `/api/folders/{id}` | Delete folder (keeps media files) |
| `GET` | `/api/folders/{id}/media` | Paginated media in folder |
| `POST` | `/api/folders/{id}/media` | Add media to folder. Body: `["uuid1", ...]` |
| `POST` | `/api/folders/{id}/media/remove` | Remove media from folder |
| `GET` | `/api/folders/{id}/download` | Download all media in folder as zip |
| `GET` | `/api/stats` | Server statistics (counts, storage, disk space) |
| `POST` | `/api/login` | Authenticate. Body: `{"password": "..."}` |
| `POST` | `/api/logout` | Clear session |
| `GET` | `/api/auth-check` | Check authentication status |

## Contributing

Contributions are welcome! Feel free to open an issue or submit a pull request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/my-feature`)
3. Commit your changes (`git commit -m 'Add my feature'`)
4. Push to the branch (`git push origin feature/my-feature`)
5. Open a Pull Request
