#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

output="${DEVDESK_RELEASE_OUT:-dist}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
stage="$(mktemp -d "${TMPDIR:-/tmp}/devdesk-core.XXXXXX")"
trap 'rm -rf "$stage"' EXIT

case "${1:-}" in
  host)
    ./scripts/dev/build-core-host.sh
    name="devdesk-core-host-$(rustc -vV | sed -n 's/^host: //p')"
    mkdir "$stage/$name"
    cp "${CARGO_TARGET_DIR:-target}/release/devdesk-host" "$stage/$name/"
    artifact="$name.tar.gz"
    tar -czf "$stage/$artifact" -C "$stage" "$name"
    ;;
  web)
    npm --prefix web ci
    npm --prefix web run build:core
    web/node_modules/.bin/tsc -p web/tsconfig.sdk.json
    name="devdesk-core-web"
    mkdir "$stage/$name"
    cp -R web/dist/core/. "$stage/$name/"
    cp schemas/domain-{package,contributions,entity-types}.schema.json "$stage/$name/sdk/"
    artifact="$name.tar.gz"
    tar -czf "$stage/$artifact" -C "$stage" "$name"
    ;;
  software)
    npm --prefix web ci
    npm --prefix web run build:software
    artifact="software-development-$(python3 configurations/software-development/build.py --version).zip"
    python3 configurations/software-development/build.py "$stage/$artifact"
    ;;
  *)
    printf 'Usage: %s host|web|software\n' "$0" >&2
    exit 2
    ;;
esac
(
  cd "$stage"
  if command -v sha256sum >/dev/null; then
    sha256sum "$artifact" > "$artifact.sha256"
  else
    shasum -a 256 "$artifact" > "$artifact.sha256"
  fi
)
mv "$stage/$artifact" "$stage/$artifact.sha256" "$output/"
printf '%s\n' "$output/$artifact"
