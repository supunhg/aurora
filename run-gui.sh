#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Install npm dependencies if needed
if [ ! -d "node_modules" ]; then
  echo "[*] Installing npm dependencies..."
  pnpm install
  pnpm approve-builds esbuild
fi

echo "[*] Launching Aurora GUI (Tauri dev)..."
pnpm tauri dev
