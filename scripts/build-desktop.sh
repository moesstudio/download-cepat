#!/usr/bin/env bash
set -e

echo "==========================================="
echo "   Download Cepat - Desktop Build Tool    "
echo "==========================================="

echo "[1/3] Building frontend assets..."
npm run build

echo "[2/3] Checking Rust engine..."
cargo test --manifest-path core-engine/Cargo.toml --quiet

echo "[3/3] Packaging native desktop installer..."
npm run desktop:build

echo ""
echo "Build complete! Check src-tauri/target/release/bundle/ for the installer."
