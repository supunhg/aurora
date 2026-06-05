#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

SIDECAR_DIR="sidecar/freellmapi"
SIDECAR_PID=""

# -----------------------------------------------------------------------
# Cleanup: kill the sidecar when this script exits
# -----------------------------------------------------------------------
cleanup() {
  if [ -n "$SIDECAR_PID" ] && kill -0 "$SIDECAR_PID" 2>/dev/null; then
    echo "[*] Stopping sidecar (PID $SIDECAR_PID)..."
    kill "$SIDECAR_PID" 2>/dev/null || true
    wait "$SIDECAR_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

# -----------------------------------------------------------------------
# 1. Install npm dependencies for the Tauri frontend if needed
# -----------------------------------------------------------------------
if [ ! -d "node_modules" ]; then
  echo "[*] Installing npm dependencies..."
  pnpm install
  pnpm approve-builds esbuild
fi

# -----------------------------------------------------------------------
# 2. Set up the FreeLLMAPI sidecar (clone + deps + .env)
# -----------------------------------------------------------------------
if ! bash sidecar/setup.sh; then
  echo "[!] Sidecar setup failed — AI features will be unavailable"
fi

# -----------------------------------------------------------------------
# 3. Start the sidecar in the background
# -----------------------------------------------------------------------
if [ -d "$SIDECAR_DIR" ]; then
  # Determine entry point: prefer built dist, fall back to tsx dev mode
  if [ -f "$SIDECAR_DIR/server/dist/index.js" ]; then
    echo "[*] Starting sidecar (production mode)..."
    (cd "$SIDECAR_DIR" && PORT=3001 node server/dist/index.js) &
  else
    echo "[*] Starting sidecar (dev mode)..."
    (cd "$SIDECAR_DIR" && PORT=3001 npx tsx server/src/index.ts) &
  fi
  SIDECAR_PID=$!
  echo "[*] Sidecar PID: $SIDECAR_PID"

  # Wait for the sidecar to become healthy (poll /api/ping)
  echo "[*] Waiting for sidecar health check..."
  for i in $(seq 1 20); do
    if ! kill -0 "$SIDECAR_PID" 2>/dev/null; then
      echo "[!] Sidecar exited unexpectedly — AI features will be unavailable"
      SIDECAR_PID=""
      break
    fi
    if curl -sf http://localhost:3001/api/ping >/dev/null 2>&1; then
      echo "[*] Sidecar ready on http://localhost:3001"
      break
    fi
    if [ "$i" -eq 20 ]; then
      echo "[!] Warning: sidecar did not respond within 10s — AI features may be unavailable"
    fi
    sleep 0.5
  done
else
  echo "[!] Sidecar directory not found — AI features will be unavailable"
fi

# -----------------------------------------------------------------------
# 4. Launch the Tauri GUI (frontend + Rust backend)
# -----------------------------------------------------------------------
echo "[*] Launching Aurora GUI (Tauri dev)..."
pnpm tauri dev
