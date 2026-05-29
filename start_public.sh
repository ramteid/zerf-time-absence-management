#!/bin/bash
set -euo pipefail

# Start the application in production mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
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

docker compose -f docker/docker-compose-local.yml -f docker/docker-compose-public.yml --env-file .env up -d "$build_flag"
