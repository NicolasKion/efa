#!/bin/bash
# Fetches the latest deimos release into data/eve and builds the data pack from it, which the
# next `cargo build` embeds. Usage: scripts/build-pack.sh [out file]
# The default out file is assets/data/efa-data.pack; build with EFA_PACK=<out file> when it is
# somewhere else.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/assets/data/efa-data.pack}

"$ROOT/scripts/fetch-data.sh"
# The builder is efa itself, which needs no pack to build one (EFA_PACK may name the file this
# script is about to write, which build.rs would refuse as missing).
env -u EFA_PACK cargo build --release --manifest-path "$ROOT/Cargo.toml"
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$ROOT/Cargo.toml" | jq -r .target_directory)
exe="$target/release/efa"
[ -x "$exe" ] || exe="$exe.exe"
"$exe" --build-pack "$ROOT/data/eve" "$OUT"
