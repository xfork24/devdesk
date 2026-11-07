#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/.."
npm --prefix web run build
cargo test --workspace --locked
cargo build --locked -p devdesk-host
python3 tests/http_contract.py
(cd web && npx playwright test)
cargo build --release --locked -p devdesk-host
python3 tests/http_contract.py target/release/devdesk-host --standalone
(cd web && DEVDESK_E2E_PROFILE=release npx playwright test)
