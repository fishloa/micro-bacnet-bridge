#!/bin/bash
# Check that all frontend API routes have matching firmware backend routes.
# Run this as part of the build/CI process.

set -e

FRONTEND_API="frontend/src/lib/api.ts"
FIRMWARE_HTTP="firmware/src/http.rs"

echo "Checking frontend API routes against firmware HTTP routes..."

# Extract GET/PUT/POST paths from frontend api.ts
FRONTEND_PATHS=$(grep -E "(get|put|post|del)\(" "$FRONTEND_API" \
  | grep -oE "'/(config|system|devices|users|tokens|auth|tls)[^']*'" \
  | sed "s/'//g" \
  | sort -u)

# Extract ALL /api/v1/* paths from firmware http.rs
FIRMWARE_PATHS=$(grep -oE '"/api/v1[^"]*"' "$FIRMWARE_HTTP" \
  | sed 's|"/api/v1||;s|"||g;s|/$||' \
  | sort -u)

MISSING=0
for path in $FRONTEND_PATHS; do
  if ! echo "$FIRMWARE_PATHS" | grep -qF "$path"; then
    echo "  MISSING: $path (frontend calls it, firmware doesn't have it)"
    MISSING=$((MISSING + 1))
  fi
done

if [ $MISSING -eq 0 ]; then
  echo "All frontend API routes have matching firmware routes. ✓"
else
  echo ""
  echo "$MISSING frontend route(s) have no firmware backend."
  echo "These will return HTML instead of JSON, causing parse errors."
  exit 1
fi
