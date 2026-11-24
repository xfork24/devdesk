#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
./scripts/check/check-permissions.sh
npm --prefix web ci
npm --prefix web run build:core
cargo build --release --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/release/devdesk-host
printf '\nRun: ./target/release/devdesk-host --web-dir web/dist/core\n'
