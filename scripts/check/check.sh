#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
./scripts/check/check-permissions.sh
python3 tests/entrypoints/permission_schema.py
./scripts/check/check-defaults.sh
./scripts/check/check-architecture.sh
python3 scripts/check/check-docs.py
python3 tests/entrypoints/test_docs_checker.py
python3 tests/entrypoints/rpc_contract_inventory.py
python3 tests/entrypoints/host_launcher.py
node --test tests/entrypoints/ci_workflow_gate.mjs tests/entrypoints/configuration_query_cache.mjs
python3 -m unittest discover -s tests/live -p 'test_*.py'
./scripts/check/check-style.sh
npm --prefix web run build:fixture
python3 tests/entrypoints/prepare_project_test_runtime.py
cargo test --workspace --locked
cargo build --locked -p devdesk-host --features fixture-catalog --target-dir target/fixtures
./scripts/platform/macos/sign-macos.sh target/fixtures/debug/devdesk-host
python3 tests/entrypoints/lifecycle_contract.py target/fixtures/debug/devdesk-host
python3 tests/entrypoints/event_tail_contract.py target/fixtures/debug/devdesk-host
python3 tests/entrypoints/http_contract.py
python3 tests/entrypoints/template_reset.py target/fixtures/debug/devdesk-host
python3 tests/entrypoints/dataset_protocol.py target/fixtures/debug/devdesk-host
python3 tests/entrypoints/runtime_preparation.py target/fixtures/debug/devdesk-host
python3 tests/entrypoints/prepare_browser_runtime.py
npm --prefix web run test:e2e
cargo build --release --locked -p devdesk-host --features fixture-catalog --target-dir target/fixtures
./scripts/platform/macos/sign-macos.sh target/fixtures/release/devdesk-host
python3 tests/entrypoints/lifecycle_contract.py target/fixtures/release/devdesk-host
python3 tests/entrypoints/event_tail_contract.py target/fixtures/release/devdesk-host
python3 tests/entrypoints/http_contract.py target/fixtures/release/devdesk-host --standalone
python3 tests/entrypoints/template_reset.py target/fixtures/release/devdesk-host
python3 tests/entrypoints/dataset_protocol.py target/fixtures/release/devdesk-host
python3 tests/entrypoints/runtime_preparation.py target/fixtures/release/devdesk-host
DEVDESK_E2E_PROFILE=release npm --prefix web run test:e2e
