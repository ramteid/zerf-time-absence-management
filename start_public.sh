#!/bin/bash

# Start the application in production mode with Docker Compose.
# Make sure to have the .env file configured with the correct environment variables.
docker compose -f docker/docker-compose-local.yml -f docker/docker-compose-public.yml --env-file .env up -d --build