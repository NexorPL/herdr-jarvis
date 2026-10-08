#!/bin/sh
# herdr [[build]] step (linux/macos): fetch the release binary for this platform, else build from source.
set -eu
REPO="NexorPL/herdr-jarvis"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target/release/jarvis"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

build() {
    echo "jarvis: $1 - building from source" >&2
    command -v cargo >/dev/null 2>&1 || { echo "jarvis: cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }
    cd "$ROOT" && cargo build --release
    exit $?
}

fetch() {
    if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"; else wget -qO "$2" "$1"; fi
}

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) TRIPLE=x86_64-unknown-linux-musl ;;
    Linux-aarch64 | Linux-arm64) TRIPLE=aarch64-unknown-linux-musl ;;
    Darwin-x86_64) TRIPLE=x86_64-apple-darwin ;;
    Darwin-arm64) TRIPLE=aarch64-apple-darwin ;;
    *) build "no prebuilt binary for $(uname -s)-$(uname -m)" ;;
esac
ASSET="jarvis-$TRIPLE"
BASE="https://github.com/$REPO/releases/download/v$VERSION"

fetch "$BASE/$ASSET" "$TMP/$ASSET" 2>/dev/null || build "no release asset $ASSET for v$VERSION"
fetch "$BASE/SHA256SUMS" "$TMP/SHA256SUMS" 2>/dev/null || build "no SHA256SUMS for v$VERSION"
EXPECTED="$(grep " [ *]$ASSET\$" "$TMP/SHA256SUMS" | cut -c1-64)"
if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP/$ASSET" | cut -c1-64)"
else
    ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | cut -c1-64)"
fi
[ -n "$EXPECTED" ] && [ "$EXPECTED" = "$ACTUAL" ] || build "checksum mismatch for $ASSET"
mkdir -p "$(dirname "$OUT")"
mv "$TMP/$ASSET" "$OUT"
chmod +x "$OUT"
echo "jarvis: installed prebuilt v$VERSION ($TRIPLE)"
