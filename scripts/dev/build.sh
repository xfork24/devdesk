#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
./scripts/check/check-permissions.sh
npm --prefix web ci
if [ "${DEVDESK_SKIP_CREDENTIAL_HELPER_INSTALL:-0}" != 1 ]; then
  ./scripts/platform/macos/install-macos-credential-helper.sh
fi
npm --prefix web run build
cargo build --release --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/release/devdesk-host
printf '\nRun: ./target/release/devdesk-host\n'
