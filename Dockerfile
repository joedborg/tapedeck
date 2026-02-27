# ── Stage 1: Build Rust backend ────────────────────────────────────────────────
FROM rust:1.87-slim-bookworm AS backend-builder

WORKDIR /build

# Install build dependencies (OpenSSL for rustls, sqlx offline mode)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from app code
COPY backend/Cargo.toml backend/Cargo.lock* ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs \
    && cargo build --release \
    && rm -r src

# Build the real app
COPY backend/src ./src
COPY backend/migrations ./migrations
# Touch main.rs so cargo knows it changed
RUN touch src/main.rs && cargo build --release


# ── Stage 2: Build Ember frontend ──────────────────────────────────────────────
FROM node:22-slim AS frontend-builder

WORKDIR /app

COPY frontend/package.json ./
RUN npm install --legacy-peer-deps

COPY frontend/ ./
RUN npm run build


# ── Stage 3: Final runtime image ──────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install get_iplayer, ffmpeg, and their runtime deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    perl \
    curl \
    wget \
    libxml-libxml-perl \
    libwww-perl \
    libmojolicious-perl \
    && rm -rf /var/lib/apt/lists/*

# Install get_iplayer — resolve the latest release tag via the GitHub API,
# then download the script directly from that tag's raw content URL.
RUN LATEST_TAG=$(curl -fsSL https://api.github.com/repos/get-iplayer/get_iplayer/releases/latest \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4) \
    && curl -fsSL \
    "https://raw.githubusercontent.com/get-iplayer/get_iplayer/${LATEST_TAG}/get_iplayer" \
    -o /usr/local/bin/get_iplayer \
    && chmod +x /usr/local/bin/get_iplayer

# Create non-root user
RUN useradd -m -u 1000 tapedeck

# App directories (including persistent get_iplayer cache)
RUN mkdir -p /app/ui/dist /data /downloads /data/iplayer-cache \
    && chown -R tapedeck:tapedeck /app /data /downloads

# Copy backend binary
COPY --from=backend-builder --chown=tapedeck /build/target/release/tapedeck /app/tapedeck

# Copy compiled frontend assets
COPY --from=frontend-builder --chown=tapedeck /app/dist /app/ui/dist

USER tapedeck
WORKDIR /app

EXPOSE 3000

ENV DATABASE_URL=/data/tapedeck.db
ENV OUTPUT_DIR=/downloads
ENV GET_IPLAYER_PATH=/usr/local/bin/get_iplayer
ENV FFMPEG_PATH=/usr/bin/ffmpeg
ENV IPLAYER_CACHE_DIR=/data/iplayer-cache
ENV STATIC_DIR=/app/ui/dist
ENV BIND=0.0.0.0:3000

ENTRYPOINT ["/app/tapedeck"]
