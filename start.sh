#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if (( $# != 1 )); then
    echo "Usage: ./start.sh <dev|build>" >&2
    exit 2
fi
case "$1" in
    dev) exec cargo run --locked --bin ShiTu ;;
    build) exec cargo build --release --locked --bin ShiTu ;;
    *) echo "Usage: ./start.sh <dev|build>" >&2; exit 2 ;;
esac
