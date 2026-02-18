#!/bin/bash
set -e

# Function to handle shutdown signals
_term() { 
  echo "Caught SIGTERM signal! Shutting down..." 
  kill -TERM "$child" 2>/dev/null
  wait "$child"
  exit 0
}

# Trap signals to ensure clean shutdown
trap _term SIGTERM SIGINT

# Start the main application in the background
./gallerynet &
child=$!

# Background process for scheduling
(
  # Wait for the application to start (15 seconds buffer)
  sleep 15
  
  # Trigger on container start
  echo "Triggering startup thumbnail fix..."
  curl -s -o /dev/null -X POST http://localhost:3000/api/media/fix-thumbnails || echo "Failed to trigger startup fix"

  # Loop for daily execution
  while true; do
    sleep 86400 # Wait 24 hours
    echo "Triggering daily thumbnail fix..."
    curl -s -o /dev/null -X POST http://localhost:3000/api/media/fix-thumbnails || echo "Failed to trigger daily fix"
  done
) &

# Wait for the main application process
wait "$child"
