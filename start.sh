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
    dev)
        exec cargo run --package "$app" --bin "$bin"
        ;;
    build)
        exec cargo build --release --package "$app" --bin "$bin"
        ;;
    *)
        echo "Unknown action: $action" >&2
        usage
        exit 2
        ;;
esac
