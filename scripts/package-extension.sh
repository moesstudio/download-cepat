#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EXT_DIR="$ROOT_DIR/extension"
OUT_DIR="$ROOT_DIR/installer"

VERSION="$(node -e "
  const fs = require('fs');
  const m = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
  if (!m.description || m.description.length > 132) {
    console.error('manifest.json description must be <= 132 chars (got ' + (m.description || '').length + ')');
    process.exit(1);
  }
  console.log(m.version);
" "$EXT_DIR/manifest.json")"

for f in background.js content.js popup.js; do
  node --check "$EXT_DIR/$f"
done

mkdir -p "$OUT_DIR"
ZIP_PATH="$OUT_DIR/download-cepat-chrome-extension-v${VERSION}.zip"
rm -f "$ZIP_PATH"

(
  cd "$EXT_DIR"
  /usr/bin/zip -r -X "$ZIP_PATH" . -x "*.DS_Store" "__MACOSX/*"
)

echo "Built Chrome Web Store package: $ZIP_PATH"
/usr/bin/unzip -l "$ZIP_PATH"
