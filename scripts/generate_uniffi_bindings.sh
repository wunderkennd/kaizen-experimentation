#!/usr/bin/env bash
set -euo pipefail

# Generate UniFFI bindings for iOS (Swift) and Android (Kotlin).
#
# Uses `cargo rustc --crate-type cdylib` to build the hash crate as a dynamic
# library without polluting the normal Cargo.toml (the crate is normally a lib).
# Then runs uniffi-bindgen to generate Swift and Kotlin source files.
#
# Usage:
#   ./scripts/generate_uniffi_bindings.sh
#
# Prerequisites:
#   cargo install uniffi-bindgen-cli

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SWIFT_OUT="$REPO_ROOT/sdks/ios/Sources/Experimentation"
KOTLIN_OUT="$REPO_ROOT/sdks/android/src/main/kotlin/com/experimentation/sdk"

echo "==> Building experimentation-hash as cdylib with uniffi feature..."
cargo rustc \
  -p experimentation-hash \
  --features uniffi \
  --crate-type cdylib \
  --manifest-path "$REPO_ROOT/crates/experimentation-hash/Cargo.toml"

# Find the built dylib (macOS: .dylib, Linux: .so).
DYLIB=$(find "$REPO_ROOT/target/debug" -maxdepth 1 \
  \( -name "libexperimentation_hash.dylib" -o -name "libexperimentation_hash.so" \) \
  -print -quit 2>/dev/null)

if [[ -z "$DYLIB" ]]; then
  echo "ERROR: Could not find libexperimentation_hash dylib in target/debug/" >&2
  exit 1
fi

echo "==> Found dylib: $DYLIB"

echo "==> Generating Swift bindings → $SWIFT_OUT"
mkdir -p "$SWIFT_OUT"
uniffi-bindgen generate \
  --library "$DYLIB" \
  --language swift \
  --out-dir "$SWIFT_OUT"

echo "==> Generating Kotlin bindings → $KOTLIN_OUT"
mkdir -p "$KOTLIN_OUT"
uniffi-bindgen generate \
  --library "$DYLIB" \
  --language kotlin \
  --out-dir "$KOTLIN_OUT"

echo "==> Done. Generated bindings:"
find "$SWIFT_OUT" -name "*.swift" -newer "$DYLIB" -print 2>/dev/null || true
find "$KOTLIN_OUT" -name "*.kt" -newer "$DYLIB" -print 2>/dev/null || true
