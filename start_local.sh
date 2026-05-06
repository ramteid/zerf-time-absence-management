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
docker compose -f docker/docker-compose-local.yml --env-file .env up -d --build

echo "App is running at http://localhost:3333 (also reachable from the LAN on port 3333)"
