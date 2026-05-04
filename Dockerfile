# syntax=docker/dockerfile:1
# ---------- Frontend build stage ----------
FROM node:25-bookworm-slim AS frontend-builder
WORKDIR /build
ENV CI=1
COPY frontend/package.json frontend/package-lock.json* ./
RUN if [ -f package-lock.json ]; then npm ci --no-audit --no-fund; \
    else npm install --no-audit --no-fund; fi
COPY frontend/ ./
RUN npm run build

# ---------- Backend build stage ----------
FROM rust:1-bookworm AS backend-builder
WORKDIR /build
ENV CARGO_TERM_COLOR=always RUSTFLAGS="-C strip=symbols"

# Layer 1: manifests only — cached until Cargo.toml / Cargo.lock change.
COPY backend/Cargo.toml backend/Cargo.lock* ./
COPY backend/migrations ./migrations

# Layer 2: compile all dependencies via a placeholder binary.
# This expensive step is re-run only when the manifest/lock changes.
RUN mkdir -p src && \
    echo 'fn main() {}' > src/main.rs && \
    cargo build --release --locked && \
    rm -f target/release/deps/kitazeit* && \
    rm -rf target/release/.fingerprint/kitazeit-*

# Layer 3: compile the real application source.
COPY backend/src ./src
RUN touch src/main.rs && \
    cargo build --release --locked && \
    strip target/release/kitazeit || true

# ---------- Runtime stage ----------
FROM debian:bookworm-slim
ARG APP_UID=10001
ARG APP_GID=10001

# Minimal runtime deps: `tini` for signal handling and `wget` for health checks.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates tini wget && \
    rm -rf /var/lib/apt/lists/*

# Non-root runtime user.
RUN groupadd --gid ${APP_GID} kitazeit && \
    useradd --uid ${APP_UID} --gid ${APP_GID} --home /app --shell /usr/sbin/nologin kitazeit

WORKDIR /app
COPY --from=backend-builder /build/target/release/kitazeit /app/kitazeit
COPY --from=frontend-builder /build/dist /app/static
RUN chmod 0555 /app/kitazeit && \
    chmod -R a=rX /app/static

ENV KITAZEIT_STATIC_DIR=/app/static \
    KITAZEIT_BIND=0.0.0.0:3000 \
    RUST_BACKTRACE=0

USER kitazeit:kitazeit
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/bin/sh", "-c", "wget -qO- --timeout=3 http://127.0.0.1:3000/healthz | grep -q ok"]

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/app/kitazeit"]
