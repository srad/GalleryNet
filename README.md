<p align="center">
  <img src="assets/logo.png" alt="GalleryNet Logo" width="120" />
</p>

<h1 align="center">GalleryNet</h1>

<p align="center">
  <strong>Self-hosted photo and video gallery with AI-powered search, grouping, and tagging</strong>
</p>

<p align="center">
  <a href="https://teamcity.sedrad.com/viewType.html?buildTypeId=GalleryNet_Build&guest=1">
    <img src="https://teamcity.sedrad.com/app/rest/builds/buildType:(id:GalleryNet_Build)/statusIcon" alt="Build Status">
  </a>

  <img src="https://img.shields.io/badge/rust-2021-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust">

  <img src="https://img.shields.io/badge/react-19-61DAFB?style=flat-square&logo=react&logoColor=white" alt="React">

  <img src="https://img.shields.io/badge/ONNX_Runtime-MobileNetV3-green?style=flat-square" alt="ONNX Runtime">

  <img src="https://img.shields.io/badge/tests-90-brightgreen?style=flat-square&logo=checkmarx&logoColor=white" alt="Tests">

  <a href="https://hub.docker.com/r/sedrad/gallerynet/tags">
    <img src="https://img.shields.io/docker/image-size/sedrad/gallerynet/v1?style=flat-square&logo=docker&logoColor=white&label=docker%20image%20size" alt="Docker Size">
  </a>

  <a href="https://github.com/srad/GalleryNet/stargazers">
    <img src="https://img.shields.io/github/stars/srad/GalleryNet?style=flat-square" alt="Stars">
  </a>
</p>

<p align="center">
  Upload photos and videos, organize them into folders and use integrated AI tools for search and sorting &mdash; all running on your own hardware.
  No cloud, no API keys, fully private. High-performance, low resource usage, and only a single container without any other dependencies.
</p>

---

<p align="center">
  <img alt="GalleryNet Screenshot" width="900" src="https://github.com/user-attachments/assets/72613ee9-ab24-4d23-94f3-9b13a6fa4836" />
</p>

---

## Features

- **Visual Search** &mdash; Find similar photos and videos by uploading a reference image, with adjustable similarity threshold and one-click grouping
- **Auto Tagging** &mdash; Tag a few examples and let the AI automatically label matching items across your library
- **Duplicate Detection** &mdash; Duplicates are detected during upload and silently skipped
- **Virtual Folders** &mdash; Organize media into folders without moving files; one item can live in multiple folders with drag-and-drop support
- **Favorites** &mdash; Mark items as favorites for quick access in a dedicated view
- **Multi-Select & Batch Operations** &mdash; Marquee selection, shift-click, batch download (auto-split zip), batch delete, and batch add-to-folder
- **Video Support** &mdash; Upload any common video format with automatic frame extraction for thumbnails and AI features
- **EXIF Metadata** &mdash; View camera details, date, GPS, exposure, and more
- **Deep Linking** &mdash; Bookmarkable URLs for folders, favorites, search states, and individual items
- **Password Protection** &mdash; Optional single-password auth with rate limiting and secure sessions
- **Responsive UI** &mdash; Infinite-scroll grid, keyboard shortcuts, touch swipe, and full mobile support
- **100% Self-Hosted** &mdash; No cloud, no telemetry. Your data stays yours.

### How It Works

| Feature | Technology |
|---------|-----------|
| Visual Search | MobileNetV3-Large extracts 1280-dim feature vectors; cosine similarity via [sqlite-vec](https://github.com/asg017/sqlite-vec) |
| Similarity Grouping | Agglomerative clustering over the same embedding space with a user-adjustable distance threshold |
| Auto-Tagging | Linear SVM with Platt-calibrated probabilities trained on user-provided examples via [linfa-svm](https://crates.io/crates/linfa-svm) |
| Duplicate Detection | Perceptual hashing ([image_hasher](https://crates.io/crates/image_hasher)) compared at upload time |
| Video Processing | ffmpeg `thumbnail` filter selects visually distinct frames for thumbnails, hashing, and embeddings |
| AI Inference | [ort](https://github.com/pykeio/ort) (ONNX Runtime) for fast CPU-based model execution |
| Batch Downloads | Real-time ZIP streaming via [async_zip](https://crates.io/crates/async_zip) with automatic partitioning into ~2 GB parts |
| Authentication | Argon2-hashed password, rate-limited login, secure HTTP-only cookies |

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
      - GALLERY_PASSWORD=your-secret-password
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
| `CORS_ORIGIN` | *(empty)* | Set to allow cross-origin requests from a specific origin (e.g. `https://example.com`). Unset = same-origin only |

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
| `POST` | `/api/media/download/plan` | Create download plan (partitions large sets into <2GB parts). Body: `["uuid1", ...]` |
| `GET` | `/api/media/download/stream/{id}` | Stream a specific download part incrementally |
| `POST` | `/api/media/download` | Simple batch download (if under 2GB). Body: `["uuid1", ...]` |
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
| `GET` | `/api/folders/{id}/download` | Get download plan for folder (auto-splits for large folders) |
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
