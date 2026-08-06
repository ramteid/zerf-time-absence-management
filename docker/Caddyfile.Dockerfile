# syntax=docker/dockerfile:1
#
# Build a Caddy binary that includes the caddy-ratelimit module so the
# per-IP edge rate limits in the project Caddyfile take effect.
#
# Two-stage build: xcaddy compiles the binary, the runtime image is the
# regular caddy:2-alpine so we keep the official entrypoint, certificate
# storage layout, HTTP/3 support and image surface.
FROM caddy:2-builder-alpine AS builder
# The Go module and build caches are BuildKit cache mounts, not layers: they
# speed up repeated builds (xcaddy compiles Caddy from source every time) but
# are never committed to any image, so `docker builder prune` /
# `docker system prune -a` reclaims them completely.
RUN --mount=type=cache,target=/go/pkg/mod,id=zerf-caddy-gomod \
    --mount=type=cache,target=/root/.cache/go-build,id=zerf-caddy-gobuild \
    xcaddy build \
    --with github.com/mholt/caddy-ratelimit

FROM caddy:2-alpine
ARG ZERF_GIT_COMMIT=unknown
LABEL org.opencontainers.image.revision="${ZERF_GIT_COMMIT}"
ENV ZERF_GIT_COMMIT=${ZERF_GIT_COMMIT}
COPY --from=builder /usr/bin/caddy /usr/bin/caddy
