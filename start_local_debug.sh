#!/bin/bash

# Start the application in debug mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
docker compose -f docker/docker-compose-local.yml -f docker/docker-compose-local-debug.yml --env-file .env up -d --build

echo "Debug app is running at http://localhost:3000"
