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
cp -R containers "$staging/$package_name/"
if [ "$(uname -s)" = Linux ]; then
  mkdir -p "$staging/$package_name/scripts"
  cp scripts/install-linux-sandbox-policy.sh "$staging/$package_name/scripts/"
fi
if [ "$(uname -s)" = Darwin ]; then
  mkdir -p "$staging/$package_name/credential-helper"
  cp "$HOME/Library/Application Support/DevDesk/credential-helper/v1/devdesk-credential-helper" "$staging/$package_name/credential-helper/"
  cp "$HOME/Library/Application Support/DevDesk/credential-helper/v1/manifest.json" "$staging/$package_name/credential-helper/"
fi
COPYFILE_DISABLE=1 tar --exclude=.DS_Store --exclude=._'*' -czf "dist/$package_name.tar.gz" -C "$staging" "$package_name"
(
  cd dist
  if command -v sha256sum >/dev/null; then
    sha256sum "$package_name.tar.gz" > "$package_name.tar.gz.sha256"
  else
    shasum -a 256 "$package_name.tar.gz" > "$package_name.tar.gz.sha256"
  fi
)
printf '\nPackage: dist/%s.tar.gz\n' "$package_name"
