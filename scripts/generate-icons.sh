#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

source_icon="assets/icons/source/pw-duck.png"
source_icon_on="assets/icons/source/pw-duck-on.png"

if ! command -v magick >/dev/null 2>&1; then
  echo "error: ImageMagick 'magick' is required. Use the dev shell or: nix shell nixpkgs#imagemagick" >&2
  exit 1
fi

if ! command -v perl >/dev/null 2>&1; then
  echo "error: perl is required to convert raw RGBA bytes to ARGB pixmaps" >&2
  exit 1
fi

if [[ ! -f "$source_icon" ]]; then
  echo "error: missing source icon: $source_icon" >&2
  exit 1
fi

if [[ ! -f "$source_icon_on" ]]; then
  echo "error: missing source icon: $source_icon_on" >&2
  exit 1
fi

# Source images:
#   assets/icons/source/pw-duck.png     Ducking OFF / app icon
#   assets/icons/source/pw-duck-on.png  Ducking ON tray icon
#
# The hicolor PNGs, historical symbolic PNG aliases, and ksni pixmap
# ARGB blobs are generated artifacts. Do not edit them by hand.
hicolor_sizes=(16 24 32 48 64 128 256 512)
pixmap_sizes=(16 22 24 32 48 64)

for size in "${hicolor_sizes[@]}"; do
  app_dir="assets/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$app_dir"

  magick "$source_icon" \
    -resize "${size}x${size}" \
    -strip \
    "$app_dir/pw-duck.png"

  # Keep the existing installed symbolic icon names for compatibility, but
  # derive them from the same source image to preserve one editable source.
  magick "$source_icon" \
    -resize "${size}x${size}" \
    -strip \
    "$app_dir/pw-duck-symbolic.png"
done

mkdir -p assets/icons/pixmap
for size in "${pixmap_sizes[@]}"; do
  magick "$source_icon" \
    -resize "${size}x${size}" \
    -depth 8 \
    RGBA:- \
    | perl -0777 -ne 'for ($i = 0; $i < length($_); $i += 4) { print substr($_, $i + 3, 1), substr($_, $i, 3) }' \
    > "assets/icons/pixmap/pw-duck-${size}.argb"

  magick "$source_icon_on" \
    -resize "${size}x${size}" \
    -depth 8 \
    RGBA:- \
    | perl -0777 -ne 'for ($i = 0; $i < length($_); $i += 4) { print substr($_, $i + 3, 1), substr($_, $i, 3) }' \
    > "assets/icons/pixmap/pw-duck-on-${size}.argb"
done

echo "generated icons from $source_icon and $source_icon_on"
