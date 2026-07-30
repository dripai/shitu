#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "Usage: $0 <binary-path> <release-label> <architecture>" >&2
    exit 2
fi

binary_path=$1
release_label=$2
architecture=$3
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output_dir="$repo_root/release-assets"

if [[ ! -x "$binary_path" ]]; then
    echo "ShiPing executable was not found or is not executable: $binary_path" >&2
    exit 1
fi

stage_root=$(mktemp -d)
trap 'rm -rf -- "$stage_root"' EXIT

bundle_dir="$stage_root/ShiPing"
mkdir -p \
    "$bundle_dir/share/applications" \
    "$bundle_dir/share/icons/hicolor/256x256/apps" \
    "$output_dir"

install -m 0755 "$binary_path" "$bundle_dir/ShiPing"
install -m 0644 \
    "$repo_root/apps/shiping/packaging/linux/README.txt" \
    "$bundle_dir/README.txt"
install -m 0644 \
    "$repo_root/apps/shiping/packaging/linux/com.dripai.shiping.desktop" \
    "$bundle_dir/share/applications/com.dripai.shiping.desktop"
install -m 0644 \
    "$repo_root/apps/shiping/assets/app.png" \
    "$bundle_dir/share/icons/hicolor/256x256/apps/com.dripai.shiping.png"

archive_name="ShiPing-${release_label}-linux-${architecture}.tar.gz"
archive_path="$output_dir/$archive_name"
tar -C "$stage_root" -czf "$archive_path" ShiPing

(
    cd "$output_dir"
    sha256sum "$archive_name" > "$archive_name.sha256"
)
