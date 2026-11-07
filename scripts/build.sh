#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/.."
npm --prefix web ci
npm --prefix web run build
cargo build --release --locked -p devdesk-host
printf '\nRun: ./target/release/devdesk-host\n'
