#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/.."
cargo test --workspace --locked
npm --prefix web run build
cargo build --locked -p devdesk-host
python3 tests/http_contract.py
(cd web && npx playwright test)
