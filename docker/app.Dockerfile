# syntax=docker/dockerfile:1
#
# ---------- Frontend build stage ----------
FROM node:26-alpine AS frontend-builder
WORKDIR /build
ARG ZERF_FRONTEND_DEBUG_BUILD=false
ENV CI=1
ENV ZERF_FRONTEND_DEBUG_BUILD=${ZERF_FRONTEND_DEBUG_BUILD}
COPY frontend/package.json frontend/package-lock.json* ./
# npm's download cache is a BuildKit cache mount, not a layer: it speeds up
# repeated builds but is never committed to the image, so it disappears with
# the rest of the build cache on `docker builder prune` / `system prune -a`.
RUN --mount=type=cache,target=/root/.npm,id=zerf-npm-cache \
    if [ -f package-lock.json ]; then npm ci --no-audit --no-fund; \
    else npm install --no-audit --no-fund; fi
COPY frontend/ ./
RUN npm run build

# ---------- Backend build stage ----------
# The slim variant still ships a C toolchain (needed to build `ring`, a
# transitive rustls dependency) but is ~750 MB smaller than the full image --
# it just drops extras (docs, extra target platforms) neither stage needs.
FROM rust:1-slim-trixie AS backend-builder
WORKDIR /build
ARG ZERF_BUILD_PROFILE=release
ENV CARGO_TERM_COLOR=always

# Layer 1: manifests only — cached until Cargo.toml / Cargo.lock change.
COPY backend/Cargo.toml backend/Cargo.lock* ./
COPY backend/migrations ./migrations

# Layer 2: compile all dependencies via a placeholder binary.
# This expensive step is re-run only when the manifest/lock changes.
#
# The downloaded crate registry and the compiled `target/` directory are
# BuildKit cache mounts, not image layers: cargo reuses them across builds to
# skip re-downloading and re-compiling unchanged dependencies, but their
# content is never baked into any committed layer. `docker builder prune` /
# `docker system prune -a` reclaims them completely, so the next build after
# a nightly prune simply starts from zero again instead of leaving a stale
# multi-hundred-MB target/ directory on disk.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=zerf-cargo-registry \
    --mount=type=cache,target=/build/target,id=zerf-cargo-target \
    mkdir -p src && \
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

# Layer 3: compile the real application source, then copy the binary out of
# the cache-mounted target/ into a normal (persisted) layer at /out -- the
# mount and everything left in it disappear the moment this RUN ends.
COPY backend/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=zerf-cargo-registry \
    --mount=type=cache,target=/build/target,id=zerf-cargo-target \
    touch src/main.rs && \
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
ARG ZERF_GIT_COMMIT=unknown
ARG ZERF_VERSION=unknown

LABEL org.opencontainers.image.revision="${ZERF_GIT_COMMIT}" \
      org.opencontainers.image.version="${ZERF_VERSION}"

# Minimal runtime deps: `tini` for signal handling and `wget` for health checks.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates tini wget && \
    rm -rf /var/lib/apt/lists/*

# Non-root runtime user.
RUN groupadd --gid ${APP_GID} zerf && \
    useradd --uid ${APP_UID} --gid ${APP_GID} --home /app --shell /usr/sbin/nologin zerf

WORKDIR /app
# --chmod sets permissions as part of the COPY itself instead of a separate
# RUN layer: a same-content RUN chmod after COPY doubles that content's size
# in the image (the layer diff captures the files again under the new mode).
COPY --from=backend-builder --chmod=0555 /out/zerf /app/zerf
# Numeric mode, not the symbolic "a=rX" form: the BuildKit build on the
# production host mishandles the conditional-execute "X" bit on a
# directory COPY and silently zeroes every copied entry's permissions
# (verified: /app/static and its contents ended up mode 000, causing the
# app to 404 on all static assets). 0555 is safe for a static file tree —
# the stray execute bit on regular files is harmless.
COPY --from=frontend-builder --chmod=0555 /build/dist /app/static

ENV ZERF_STATIC_DIR=/app/static \
    ZERF_BIND=0.0.0.0:3333 \
    ZERF_GIT_COMMIT=${ZERF_GIT_COMMIT} \
    RUST_BACKTRACE=0

USER zerf:zerf
EXPOSE 3333
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/bin/sh", "-c", "wget -qO- --timeout=3 http://127.0.0.1:3333/healthz | grep -q ok"]

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/app/zerf"]
