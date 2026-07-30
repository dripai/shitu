#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "Usage: $0 <binary-path> <release-label> <architecture>" >&2
    exit 2
fi

binary_path=$1
release_label=$2
architecture=$3
version=${release_label#v}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output_dir="$repo_root/release-assets"

if [[ ! -x "$binary_path" ]]; then
    echo "ShiPing executable was not found or is not executable: $binary_path" >&2
    exit 1
fi

stage_root=$(mktemp -d)
trap 'rm -rf -- "$stage_root"' EXIT

app_dir="$stage_root/ShiPing.app"
contents_dir="$app_dir/Contents"
resources_dir="$contents_dir/Resources"
mkdir -p "$contents_dir/MacOS" "$resources_dir" "$output_dir"

install -m 0755 "$binary_path" "$contents_dir/MacOS/ShiPing"
install -m 0644 \
    "$repo_root/apps/shiping/packaging/macos/Info.plist" \
    "$contents_dir/Info.plist"

/usr/libexec/PlistBuddy \
    -c "Set :CFBundleShortVersionString $version" \
    -c "Set :CFBundleVersion $version" \
    "$contents_dir/Info.plist"

iconset_dir="$stage_root/AppIcon.iconset"
mkdir -p "$iconset_dir"

while read -r size filename; do
    sips -z "$size" "$size" \
        "$repo_root/apps/shiping/assets/app.png" \
        --out "$iconset_dir/$filename" >/dev/null
done <<'SIZES'
16 icon_16x16.png
32 icon_16x16@2x.png
32 icon_32x32.png
64 icon_32x32@2x.png
128 icon_128x128.png
256 icon_128x128@2x.png
256 icon_256x256.png
512 icon_256x256@2x.png
512 icon_512x512.png
1024 icon_512x512@2x.png
SIZES

iconutil -c icns "$iconset_dir" -o "$resources_dir/AppIcon.icns"
plutil -lint "$contents_dir/Info.plist"

# An ad-hoc signature verifies the bundle structure but does not replace a
# Developer ID signature or Apple notarization.
codesign --force --deep --sign - --timestamp=none "$app_dir"
codesign --verify --deep --strict "$app_dir"

archive_name="ShiPing-${release_label}-macos-${architecture}-unsigned.zip"
archive_path="$output_dir/$archive_name"
ditto -c -k --sequesterRsrc --keepParent "$app_dir" "$archive_path"

(
    cd "$output_dir"
    shasum -a 256 "$archive_name" > "$archive_name.sha256"
)
