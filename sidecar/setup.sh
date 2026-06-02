#!/bin/bash
# Setup script for FreeLLMAPI sidecar
# Run this once to install dependencies and generate keys.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SIDECAR_DIR="$SCRIPT_DIR/freellmapi"

# Clone if not present
if [ ! -d "$SIDECAR_DIR" ]; then
    echo "[sidecar] Cloning FreeLLMAPI..."
    git clone --depth 1 https://github.com/tashfeenahmed/freellmapi.git "$SIDECAR_DIR"
fi

# Install dependencies
echo "[sidecar] Installing npm dependencies..."
cd "$SIDECAR_DIR"
npm install

# Generate .env if not present
if [ ! -f "$SIDECAR_DIR/.env" ]; then
    echo "[sidecar] Generating .env..."
    cp .env.example .env
    ENCRYPTION_KEY=$(node -e "console.log(require('crypto').randomBytes(32).toString('hex'))")
    echo "ENCRYPTION_KEY=$ENCRYPTION_KEY" >> .env
    echo "PORT=3001" >> .env
fi

echo "[sidecar] Setup complete."
echo "  Start:  cd $SIDECAR_DIR && npm run dev"
echo "  Build:  cd $SIDECAR_DIR && npm run build"
echo "  API:    http://localhost:3001/v1/chat/completions"
echo "  Admin:  http://localhost:5173"
