#!/usr/bin/env bash
# Web development entry: every macOS rebuild receives the configured stable identity.
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
if [ ! -d web/node_modules ]; then npm --prefix web ci; fi
./scripts/platform/macos/install-macos-credential-helper.sh --ensure
npm --prefix web run build
cargo build --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/debug/devdesk-host
exec python3 scripts/dev/run-host.py target/debug/devdesk-host "$@"
