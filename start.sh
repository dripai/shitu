#!/usr/bin/env bash

set -euo pipefail

usage() {
    echo "Usage: ./start.sh <dev|build> [shitu|shiping|shiyin]" >&2
}

if (( $# < 1 || $# > 2 )); then
    usage
    exit 2
fi

action="$1"
app="${2:-shitu}"

case "$app" in
    shitu)
        bin="ShiTu"
        ;;
    shiping | shiyin)
        bin="$app"
        ;;
    *)
        echo "Unknown application: $app" >&2
        usage
        exit 2
        ;;
esac

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

case "$action" in
    dev | build)
        ;;
    *)
        echo "Unknown action: $action" >&2
        usage
        exit 2
        ;;
esac

verify_sha256() {
    local file="$1"
    local expected="$2"

    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s  %s\n' "$expected" "$file" | sha256sum --check --status
    elif command -v shasum >/dev/null 2>&1; then
        [[ "$(shasum -a 256 "$file" | awk '{ print $1 }')" == "$expected" ]]
    else
        echo "Neither sha256sum nor shasum is available for Skia archive verification." >&2
        return 1
    fi
}

prepare_skia_binaries() {
    if [[ -n "${SKIA_BINARIES_URL:-}" ]]; then
        echo "Using SKIA_BINARIES_URL provided by the environment"
        return
    fi

    local supported_skia_version="0.99.0"
    local skia_version
    local skia_revision="a25a0fdb7d90429aa2d1"
    local platform
    local expected_sha256
    local kernel
    local machine

    skia_version="$(
        awk '
            $0 == "[[package]]" { in_package = 0 }
            $0 == "name = \"skia-bindings\"" { in_package = 1; next }
            in_package && $1 == "version" {
                gsub(/"/, "", $3)
                print $3
                exit
            }
        ' Cargo.lock
    )"
    if [[ -z "$skia_version" ]]; then
        echo "Unable to determine the locked skia-bindings version from Cargo.lock." >&2
        exit 1
    fi
    if [[ "$skia_version" != "$supported_skia_version" ]]; then
        echo "Unsupported skia-bindings version: $skia_version." >&2
        echo "Update the Skia revision, platform mappings, and SHA-256 values in start.sh." >&2
        exit 1
    fi

    kernel="$(uname -s)"
    machine="$(uname -m)"

    case "$kernel" in
        MINGW* | MSYS* | CYGWIN*)
            case "$machine" in
                x86_64 | amd64)
                    platform="x86_64-pc-windows-msvc-d3d-gl-jpegd-jpege-pdf-textlayout"
                    expected_sha256="9e6c3d1da63ae202bff9938329ccaf81afc24acb4193aec15d6f0aac72a5960f"
                    ;;
                aarch64 | arm64)
                    platform="aarch64-pc-windows-msvc-d3d-gl-jpegd-jpege-pdf-textlayout"
                    expected_sha256="66a3731c3a9487f9cfd762df9cc8535724740670836083d5c7044337b68d4af9"
                    ;;
            esac
            ;;
        Darwin)
            case "$machine" in
                x86_64 | amd64)
                    platform="x86_64-apple-darwin-gl-jpegd-jpege-metal-pdf-textlayout"
                    expected_sha256="f389ab4aca031a96294fa396b614286035c23c2762f022cca1dc061977ba3ae4"
                    ;;
                aarch64 | arm64)
                    platform="aarch64-apple-darwin-gl-jpegd-jpege-metal-pdf-textlayout"
                    expected_sha256="1169e56adba14bf37c9b7890cd4ed1b85ec1a3f461f9083932ba6fae153090d0"
                    ;;
            esac
            ;;
        Linux)
            case "$machine" in
                x86_64 | amd64)
                    platform="x86_64-unknown-linux-gnu-gl-jpegd-jpege-pdf-textlayout-vulkan"
                    expected_sha256="097e78d775c9156dc4b070b9cca7008dbab587513ecb1924baf4cf9620f3119b"
                    ;;
                aarch64 | arm64)
                    platform="aarch64-unknown-linux-gnu-gl-jpegd-jpege-pdf-textlayout-vulkan"
                    expected_sha256="ffe0e2e22113c0eee5699187943e24bec8bc85b13411102101fee132ac96f42b"
                    ;;
            esac
            ;;
    esac

    if [[ -z "${platform:-}" ]]; then
        echo "No bundled Skia download mapping for $kernel/$machine." >&2
        echo "Set SKIA_BINARIES_URL explicitly for this target." >&2
        exit 2
    fi

    local file_name="skia-binaries-${skia_revision}-${platform}.tar.gz"
    local cache_dir="$PWD/download/skia/$skia_version"
    local archive="$cache_dir/$file_name"
    local partial="$archive.part"
    local url="https://github.com/rust-skia/skia-binaries/releases/download/$skia_version/$file_name"

    mkdir -p "$cache_dir"

    if [[ -f "$archive" ]]; then
        if ! verify_sha256 "$archive" "$expected_sha256"; then
            echo "Cached Skia archive failed SHA-256 verification: $archive" >&2
            echo "Remove the invalid file and retry." >&2
            exit 1
        fi
        rm -f -- "$partial"
    else
        echo "Downloading Skia binaries for $platform"
        local curl_args=(
            --location
            --fail
            --show-error
            --retry 3
            --continue-at -
            --output "$partial"
        )
        case "$kernel" in
            MINGW* | MSYS* | CYGWIN*)
                curl_args+=(--ssl-revoke-best-effort)
                ;;
        esac
        curl "${curl_args[@]}" "$url"

        if ! verify_sha256 "$partial" "$expected_sha256"; then
            echo "Downloaded Skia archive failed SHA-256 verification: $partial" >&2
            exit 1
        fi
        mv -- "$partial" "$archive"
    fi

    case "$kernel" in
        MINGW* | MSYS* | CYGWIN*)
            export SKIA_BINARIES_URL="file://$(cygpath -m "$archive")"
            ;;
        *)
            export SKIA_BINARIES_URL="file://$archive"
            ;;
    esac

    echo "Using cached Skia binaries: $archive"
}

prepare_skia_binaries

export SHITU_BUILD_DATE="${SHITU_BUILD_DATE:-$(date -u +%Y-%m-%d)}"

case "$action" in
    dev)
        exec cargo run --package "$app" --bin "$bin"
        ;;
    build)
        exec cargo build --release --package "$app" --bin "$bin"
        ;;
esac
