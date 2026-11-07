#!/bin/bash
# Downloads the latest deimos release (EVE static data in Phobos layout plus the raw SDE) into
# data/eve, replacing whatever is there.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST="$ROOT/data/eve"
API=https://api.github.com/repos/NicolasKion/deimos/releases/latest

# CI passes its token; unauthenticated API calls share a small rate limit per IP.
auth=()
if [ -n "${GITHUB_TOKEN:-}" ]; then
    auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
fi
release=$(curl -fsSL "${auth[@]}" "$API")
tag=$(jq -r .tag_name <<<"$release")
url=$(jq -r '.assets[] | select(.name | endswith(".tar.zst")) | .browser_download_url' <<<"$release")

if [ -f "$DEST/VERSION" ] && [ "$(cat "$DEST/VERSION")" = "$tag" ]; then
    echo "data is already at $tag"
    exit 0
fi

echo "downloading $tag"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fL --progress-bar -o "$tmp/data.tar.zst" "$url"
mkdir -p "$tmp/eve"
tar --zstd -xf "$tmp/data.tar.zst" -C "$tmp/eve"
echo "$tag" >"$tmp/eve/VERSION"

rm -rf "$DEST"
mkdir -p "$(dirname "$DEST")"
mv "$tmp/eve" "$DEST"
echo "data is at $tag"
