#!/bin/bash

# Start the application in production mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
docker compose -f docker/docker-compose-local.yml --env-file .env up -d --build

echo "App is running at http://localhost:3000"