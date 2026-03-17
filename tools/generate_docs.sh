#!/usr/bin/env bash
# generate_docs.sh — Rebuild project documentation screenshots.
#
# Starts the SvelteKit dev server, captures screenshots of each page
# using Playwright, creates an animated GIF of the dashboard showing
# live SSE value updates, then stops the dev server.
#
# Prerequisites:
#   - bun (frontend deps installed)
#   - npx playwright (or bunx playwright)
#   - ffmpeg (for animated GIF)
#
# Usage:
#   ./tools/generate_docs.sh

set -euo pipefail
cd "$(dirname "$0")/.."

SCREENSHOTS_DIR="docs/screenshots"
FRAMES_DIR="/tmp/bacnet-bridge-gif-frames"
PORT=5179  # Use a non-standard port to avoid conflicts
DEV_SERVER_PID=""

mkdir -p "$SCREENSHOTS_DIR"
mkdir -p "$FRAMES_DIR"

cleanup() {
    if [ -n "$DEV_SERVER_PID" ]; then
        kill "$DEV_SERVER_PID" 2>/dev/null || true
        wait "$DEV_SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$FRAMES_DIR"
}
trap cleanup EXIT

echo "==> Building frontend..."
cd frontend
bun install --frozen-lockfile 2>/dev/null || bun install
bun run build

echo "==> Starting dev server on port $PORT..."
bun run dev --port "$PORT" &
DEV_SERVER_PID=$!
cd ..

# Wait for dev server to be ready
echo -n "    Waiting for server"
for i in $(seq 1 30); do
    if curl -s -o /dev/null -w '' "http://localhost:$PORT/" 2>/dev/null; then
        echo " ready!"
        break
    fi
    echo -n "."
    sleep 1
done

BASE="http://localhost:$PORT"

# Use Playwright CLI to capture screenshots
# Install if needed
npx playwright install chromium 2>/dev/null || true

PLAYWRIGHT_SCRIPT=$(cat <<'PYEOF'
const { chromium } = require('playwright');

(async () => {
    const port = process.env.PORT || '5179';
    const base = `http://localhost:${port}`;
    const dir = process.env.SCREENSHOTS_DIR || 'docs/screenshots';
    const framesDir = process.env.FRAMES_DIR || '/tmp/bacnet-bridge-gif-frames';

    const browser = await chromium.launch();
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const page = await context.newPage();

    // Dashboard
    console.log('  Capturing dashboard...');
    await page.goto(base + '/');
    await page.waitForTimeout(3000); // Let SSE populate values
    await page.screenshot({ path: `${dir}/dashboard.png` });

    // Capture GIF frames (6 frames, 1.5s apart)
    console.log('  Capturing animated frames...');
    for (let i = 1; i <= 6; i++) {
        await page.waitForTimeout(1500);
        await page.screenshot({ path: `${framesDir}/frame${i}.png` });
    }

    // Config
    console.log('  Capturing config...');
    await page.goto(base + '/config');
    await page.waitForTimeout(1000);
    await page.screenshot({ path: `${dir}/config.png` });

    // Users
    console.log('  Capturing users...');
    await page.goto(base + '/users');
    await page.waitForTimeout(1000);
    await page.screenshot({ path: `${dir}/users.png` });

    // Status
    console.log('  Capturing status...');
    await page.goto(base + '/status');
    await page.waitForTimeout(1000);
    await page.screenshot({ path: `${dir}/status.png` });

    await browser.close();
    console.log('  Screenshots saved to ' + dir);
})();
PYEOF
)

echo "==> Capturing screenshots with Playwright..."
PORT=$PORT SCREENSHOTS_DIR=$SCREENSHOTS_DIR FRAMES_DIR=$FRAMES_DIR \
    node -e "$PLAYWRIGHT_SCRIPT"

echo "==> Creating animated GIF..."
if command -v ffmpeg &>/dev/null; then
    ffmpeg -y -framerate 0.7 \
        -i "$FRAMES_DIR/frame%d.png" \
        -vf "scale=1280:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer" \
        "$SCREENSHOTS_DIR/dashboard-live.gif" 2>/dev/null
    echo "  Created dashboard-live.gif"
else
    echo "  WARNING: ffmpeg not found, skipping animated GIF"
fi

echo "==> Documentation generated:"
ls -lh "$SCREENSHOTS_DIR"/*.{png,gif} 2>/dev/null
echo "Done."
