#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
./scripts/check/check-permissions.sh
python3 tests/entrypoints/permission_schema.py
./scripts/check/check-defaults.sh
./scripts/check/check-architecture.sh
python3 tests/entrypoints/rpc_contract_inventory.py
node --test tests/entrypoints/ci_workflow_gate.mjs
./scripts/check/check-style.sh
cargo test --workspace --locked
cargo build --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/debug/devdesk-host
python3 tests/entrypoints/http_contract.py
python3 tests/entrypoints/template_reset.py target/debug/devdesk-host
python3 tests/entrypoints/dataset_protocol.py target/debug/devdesk-host
python3 tests/entrypoints/runtime_preparation.py target/debug/devdesk-host
python3 tests/entrypoints/prepare_browser_runtime.py
(cd web && npx playwright test)
cargo build --release --locked -p devdesk-host
./scripts/platform/macos/sign-macos.sh target/release/devdesk-host
python3 tests/entrypoints/http_contract.py target/release/devdesk-host --standalone
python3 tests/entrypoints/template_reset.py target/release/devdesk-host
python3 tests/entrypoints/dataset_protocol.py target/release/devdesk-host
python3 tests/entrypoints/runtime_preparation.py target/release/devdesk-host
(cd web && DEVDESK_E2E_PROFILE=release npx playwright test)
