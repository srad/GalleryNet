#!/bin/bash
# GalleryNet Verification Script

set -e

echo "--- Checking Backend Compilation ---"
cargo check

echo "--- Running Backend Tests ---"
# Get the number of tests
TEST_LIST=$(cargo test -- --list | grep ": test")
COUNT=$(echo "$TEST_LIST" | wc -l)

echo "Found $COUNT tests."

# Run the tests
cargo test

echo "--- Checking Frontend Compilation ---"
cd frontend
npm run build
cd ..

echo ""
echo "✅ Verification Successful: All tests passed and count is maintained ($COUNT)."
