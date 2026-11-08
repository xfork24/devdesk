#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/.."
./scripts/check-permissions.sh
npm --prefix web ci
./scripts/install-macos-credential-helper.sh
npm --prefix web run build
cargo build --release --locked -p devdesk-host
./scripts/sign-macos.sh target/release/devdesk-host
printf '\nRun: ./target/release/devdesk-host\n'
