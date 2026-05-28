#!/bin/bash
set -euo pipefail

# Start the application in production mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
if [ -z "${ZERF_GIT_COMMIT:-}" ] && git_commit="$(git rev-parse --verify HEAD 2>/dev/null)"; then
  export ZERF_GIT_COMMIT="$git_commit"
fi

docker compose -f docker/docker-compose-local.yml -f docker/docker-compose-public.yml --env-file .env up -d --build
