#!/bin/bash
set -euo pipefail

# Start the application in debug mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
#
# Same network behaviour as start_local.sh: published on 0.0.0.0:3333 and
# reachable from any LAN device at http://<this-host-ip>:3333.
docker compose -f docker/docker-compose-local.yml -f docker/docker-compose-local-debug.yml --env-file .env up -d --build

echo "Debug app is running at http://localhost:3333 (also reachable from the LAN on port 3333)"
