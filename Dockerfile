# Stage 1: Frontend builder
FROM node:22-slim AS frontend-builder

WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm ci
COPY frontend/ ./
RUN npm test
RUN npm run build

# Stage 2: Rust builder
FROM ubuntu:24.04 AS builder

# Prevent interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install build dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

# Copy dependency files first to cache dependencies
COPY Cargo.toml Cargo.lock ./
# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy source code
COPY . .

# Build the application
# We need to touch the main file to ensure a rebuild
RUN touch src/main.rs
RUN cargo test
RUN cargo build --release

# Find the downloaded onnxruntime library (libonnxruntime.so*)
# It is typically in target/release/build/ort-.../out/
RUN find target/release/build -name "libonnxruntime*.so*" -type f -exec cp {} /app/ \;

# Stage 3: Runtime
FROM ubuntu:24.04

WORKDIR /app

# Install runtime dependencies (OpenSSL, etc.)
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary
COPY --from=builder /app/target/release/gallerynet /app/gallerynet

# Copy the ONNX Runtime library
COPY --from=builder /app/libonnxruntime*.so* /app/

# Copy assets (model)
COPY assets /app/assets

# Copy the built frontend
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Set library path so the app finds onnxruntime
ENV LD_LIBRARY_PATH=/app

# Authentication — set GALLERY_PASSWORD to enable login protection
# Leave empty or unset to run without authentication
ENV GALLERY_PASSWORD=""

EXPOSE 3000

CMD ["./gallerynet"]
