# syntax=docker/dockerfile:1
#
# ---------- Frontend build stage ----------
FROM node:25-trixie-slim AS frontend-builder
WORKDIR /build
ARG ZERF_FRONTEND_DEBUG_BUILD=false
ENV CI=1
ENV ZERF_FRONTEND_DEBUG_BUILD=${ZERF_FRONTEND_DEBUG_BUILD}
COPY frontend/package.json frontend/package-lock.json* ./
RUN if [ -f package-lock.json ]; then npm ci --no-audit --no-fund; \
    else npm install --no-audit --no-fund; fi
COPY frontend/ ./
RUN npm run build

# ---------- Backend build stage ----------
FROM rust:1-trixie AS backend-builder
WORKDIR /build
ARG ZERF_BUILD_PROFILE=release
ENV CARGO_TERM_COLOR=always

# Layer 1: manifests only — cached until Cargo.toml / Cargo.lock change.
COPY backend/Cargo.toml backend/Cargo.lock* ./
COPY backend/migrations ./migrations

# Layer 2: compile all dependencies via a placeholder binary.
# This expensive step is re-run only when the manifest/lock changes.
RUN mkdir -p src && \
        echo 'fn main() {}' > src/main.rs && \
        if [ "$ZERF_BUILD_PROFILE" = "debug" ]; then \
            cargo build --locked && \
            rm -f target/debug/deps/zerf* && \
            rm -rf target/debug/.fingerprint/zerf-*; \
        else \
            cargo build --release --locked && \
            rm -f target/release/deps/zerf* && \
            rm -rf target/release/.fingerprint/zerf-*; \
        fi

# Layer 3: compile the real application source.
COPY backend/src ./src
RUN touch src/main.rs && \
        if [ "$ZERF_BUILD_PROFILE" = "debug" ]; then \
            cargo build --locked && \
            install -D target/debug/zerf /out/zerf; \
        else \
            cargo build --release --locked && \
            strip target/release/zerf || true && \
            install -D target/release/zerf /out/zerf; \
        fi

# ---------- Runtime stage ----------
FROM debian:trixie-slim
ARG APP_UID=10001
ARG APP_GID=10001

# Minimal runtime deps: `tini` for signal handling and `wget` for health checks.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates tini wget && \
    rm -rf /var/lib/apt/lists/*

# Non-root runtime user.
RUN groupadd --gid ${APP_GID} zerf && \
    useradd --uid ${APP_UID} --gid ${APP_GID} --home /app --shell /usr/sbin/nologin zerf

WORKDIR /app
COPY --from=backend-builder /out/zerf /app/zerf
COPY --from=frontend-builder /build/dist /app/static
RUN chmod 0555 /app/zerf && \
    chmod -R a=rX /app/static

ENV ZERF_STATIC_DIR=/app/static \
    ZERF_BIND=0.0.0.0:3333 \
    RUST_BACKTRACE=0

USER zerf:zerf
EXPOSE 3333
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/bin/sh", "-c", "wget -qO- --timeout=3 http://127.0.0.1:3333/healthz | grep -q ok"]

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/app/zerf"]
