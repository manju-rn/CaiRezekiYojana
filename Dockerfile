# CaiRezekiYojana ships as one image: the Rust binary serves both the API and
# the built React app, so there is a single process, a single port, and no
# reverse-proxy wiring to get wrong. The database is Supabase (Postgres), so
# the image holds no state.
#
#   docker build -t cairezeki:latest .
#   docker run -p 8080:8080 -e CAIREZEKI_DATABASE_URL="postgresql://..." cairezeki:latest
#
# Build images are pinned by digest. A floating tag like rust:1-bookworm is
# republished every Rust release and every Debian patch, and each new digest
# invalidates every layer below it - including the ~4 minute dependency
# compile. Bump the digests deliberately, when a newer toolchain is wanted:
#
#   docker buildx imagetools inspect rust:1-bookworm | grep Digest

# ---------------------------------------------------------------------------
# 1. Build the React app
# ---------------------------------------------------------------------------
FROM node:22-alpine@sha256:0a7108bf6c7bf5de370ffb1a3ed6be93d405b43ff159f681a8d18c0e2bc2e402 AS ui

WORKDIR /ui

# Dependencies change far less often than source, so they get their own layer.
COPY frontend/package*.json ./
RUN npm install --no-audit --no-fund

COPY frontend/ ./
RUN npm run build

# ---------------------------------------------------------------------------
# 2. Build the Rust binary
# ---------------------------------------------------------------------------
FROM rust:1-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS server

# libsqlite3-sys compiles SQLite from source, so a C toolchain is required.
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential pkg-config \
 && rm -rf /var/lib/apt/lists/*

# Mirrors the repo layout, so backend code can reach ../VERSION.
WORKDIR /build/backend

# Compile the dependency graph against a stub binary first. Editing the
# application source then rebuilds in seconds instead of minutes. This layer
# lives in a build stage, so a CI build only reuses it when the cache is
# exported with mode=max - inline cache keeps the final stage only.
COPY backend/Cargo.* ./
RUN mkdir -p src \
 && echo 'fn main() {}' > src/main.rs \
 && echo '' > src/lib.rs \
 && cargo build --release \
 && rm -rf src

COPY VERSION /build/VERSION
COPY backend/src ./src
# Cargo skips a rebuild when mtimes look unchanged; touching forces it.
RUN touch src/main.rs src/lib.rs \
 && cargo build --release

# ---------------------------------------------------------------------------
# 3. Runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1001 --create-home --home-dir /home/cairezeki cairezeki

WORKDIR /app

COPY --from=server /build/backend/target/release/cairezeki /usr/local/bin/cairezeki
COPY --from=ui /ui/dist /app/static

# Nothing is kept on disk: the database is Supabase. /data is only there for a
# local run on SQLite (CAIREZEKI_DATABASE_URL=sqlite:///data/cairezeki.db?mode=rwc).
RUN mkdir -p /data && chown -R cairezeki:cairezeki /data /app
VOLUME ["/data"]

# The database URL is a secret and is not set here: CAIREZEKI_DATABASE_URL.
ENV CAIREZEKI_BIND="0.0.0.0:8080" \
    CAIREZEKI_STATIC_DIR="/app/static" \
    CAIREZEKI_AUTO_MIGRATE="true" \
    CAIREZEKI_LOG_DIR="none" \
    CAIREZEKI_UPLOADS_DIR="none" \
    CAIREZEKI_LOG="cairezeki=info,tower_http=warn,info"

USER cairezeki
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/api/v1/health || exit 1

# tini reaps zombies and forwards SIGTERM, so graceful shutdown actually works.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["cairezeki", "serve"]
