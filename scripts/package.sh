#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/.."
./scripts/build.sh
package_name="devdesk-0.2.0-$(rustc -vV | sed -n 's/^host: //p')"
staging="$(mktemp -d "${TMPDIR:-/tmp}/devdesk-package.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/$package_name" dist
cp target/release/devdesk-host "$staging/$package_name/"
cp docs/run-binary.md "$staging/$package_name/README.md"
cp -R docs "$staging/$package_name/"
COPYFILE_DISABLE=1 tar -czf "dist/$package_name.tar.gz" -C "$staging" "$package_name"
(
  cd dist
  if command -v sha256sum >/dev/null; then
    sha256sum "$package_name.tar.gz" > "$package_name.tar.gz.sha256"
  else
    shasum -a 256 "$package_name.tar.gz" > "$package_name.tar.gz.sha256"
  fi
)
printf '\nPackage: dist/%s.tar.gz\n' "$package_name"
