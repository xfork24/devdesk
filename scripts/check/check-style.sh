#!/usr/bin/env bash
# Style gate: rustfmt + clippy (Rust) and eslint + prettier (web).
# Findings and conventions live in docs/crates/standards/rust.md and
# docs/crates/standards/web.md.
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."

cargo fmt --all -- --check
npm --prefix web run lint
npm --prefix web run typecheck
npm --prefix web run format:check
npm --prefix web run build
cargo clippy --workspace --all-targets --locked -- -D warnings
echo "style: OK — rustfmt, clippy, eslint, typescript, prettier, web build"
