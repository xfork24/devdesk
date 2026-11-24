#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

stage="$(mktemp -d "${TMPDIR:-/tmp}/devdesk-core.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
case "${1:-}" in
  host)
    ./scripts/dev/build-core-host.sh
    name="devdesk-core-host-$(rustc -vV | sed -n 's/^host: //p')"
    mkdir "$stage/$name"
    cp "${CARGO_TARGET_DIR:-target}/release/devdesk-host" "$stage/$name/"
    ;;
  web)
    npm --prefix web ci
    npm --prefix web run build:core
    name="devdesk-core-web"
    mkdir "$stage/$name"
    cp -R web/dist/core/. "$stage/$name/"
    ;;
  *)
    printf 'Usage: %s host|web\n' "$0" >&2
    exit 2
    ;;
esac
mkdir -p dist
tar -czf "dist/$name.tar.gz" -C "$stage" "$name"
printf '%s\n' "dist/$name.tar.gz"
