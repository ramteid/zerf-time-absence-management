#!/bin/bash
set -euo pipefail

# Start the application in production mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
#
# The app is published on 0.0.0.0:3333, so it is reachable from any device
# on the same LAN at http://<this-host-ip>:3333. Origin enforcement is
# disabled in this mode so any LAN address works without extra configuration;
# CSRF tokens are still enforced. Use start_public.sh for HTTPS deployments
# with strict origin enforcement.
if [ -z "${ZERF_GIT_COMMIT:-}" ] && git_commit="$(git rev-parse --verify HEAD 2>/dev/null)"; then
  export ZERF_GIT_COMMIT="$git_commit"
fi

# Version precedence: argument > .env > default (latest).
# dev → build from local Dockerfiles; anything else → use registry image.
if [ -n "${1:-}" ]; then
  export ZERF_VERSION="$1"
else
  ZERF_VERSION="$(grep -E '^ZERF_VERSION=' .env 2>/dev/null | cut -d= -f2 || true)"
  export ZERF_VERSION="${ZERF_VERSION:-latest}"
fi

if [ "$ZERF_VERSION" = "dev" ]; then
  build_flag="--build"
else
  # --pull always: refresh the registry image even when a local copy is cached.
  # --no-build: do not fall back to building from local Dockerfiles.
  build_flag="--no-build --pull always"
fi

# DEBUG=true → debug build profile, unminified frontend, RUST_BACKTRACE=1.
debug="$(grep -E '^DEBUG=' .env 2>/dev/null | cut -d= -f2 || true)"
if [ "${debug:-false}" = "true" ]; then
  export ZERF_BUILD_PROFILE=debug
  export ZERF_FRONTEND_DEBUG_BUILD=true
  export RUST_BACKTRACE=1
fi

docker compose -f docker/docker-compose-local.yml --env-file .env up -d "$build_flag"

echo "App is running at http://localhost:3333 (also reachable from the LAN on port 3333)"
