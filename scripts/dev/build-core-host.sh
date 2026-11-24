#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release --locked -p devdesk-host --no-default-features
./scripts/platform/macos/sign-macos.sh "${CARGO_TARGET_DIR:-target}/release/devdesk-host"
