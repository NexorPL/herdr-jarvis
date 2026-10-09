#!/bin/sh
# Cargo.toml and herdr-plugin.toml must carry the same version, and a release tag (if given) must match it:
# the installer downloads the release binary for Cargo.toml's version.
set -eu
cargo=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
plugin=$(sed -n 's/^version = "\(.*\)"/\1/p' herdr-plugin.toml | head -n 1)
if [ "$cargo" != "$plugin" ]; then
    echo "Cargo.toml has version $cargo but herdr-plugin.toml has $plugin" >&2
    exit 1
fi
if [ -n "${1:-}" ] && [ "$1" != "v$cargo" ]; then
    echo "tag $1 does not match version $cargo; bump Cargo.toml, Cargo.lock and herdr-plugin.toml first" >&2
    exit 1
fi
echo "version $cargo"
