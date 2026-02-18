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

# Start the main application
# The background thumbnail fix is now handled internally by the Rust application
./gallerynet &
child=$!

# Wait for the main application process
wait "$child"
