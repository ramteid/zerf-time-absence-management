# Build a Caddy binary that includes the caddy-ratelimit module so the
# per-IP edge rate limits in the project Caddyfile take effect.
#
# Two-stage build: xcaddy compiles the binary, the runtime image is the
# regular caddy:2-alpine so we keep the official entrypoint, certificate
# storage layout, HTTP/3 support and image surface.
FROM caddy:2-builder-alpine AS builder
RUN xcaddy build \
    --with github.com/mholt/caddy-ratelimit

FROM caddy:2-alpine
ARG ZERF_GIT_COMMIT=unknown
LABEL org.opencontainers.image.revision="${ZERF_GIT_COMMIT}"
ENV ZERF_GIT_COMMIT=${ZERF_GIT_COMMIT}
COPY --from=builder /usr/bin/caddy /usr/bin/caddy
