#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
./scripts/check/check-permissions.sh
./scripts/check/check-defaults.sh
npm --prefix web run build
cargo test --workspace --locked
cargo build --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/debug/devdesk-host
python3 tests/entrypoints/http_contract.py
python3 tests/entrypoints/template_reset.py target/debug/devdesk-host
python3 tests/entrypoints/dataset_protocol.py target/debug/devdesk-host
python3 tests/entrypoints/runtime_preparation.py
(cd web && npx playwright test)
cargo build --release --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/release/devdesk-host
python3 tests/entrypoints/http_contract.py target/release/devdesk-host --standalone
python3 tests/entrypoints/template_reset.py target/release/devdesk-host
(cd web && DEVDESK_E2E_PROFILE=release npx playwright test)
