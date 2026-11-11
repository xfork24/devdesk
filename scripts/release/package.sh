#!/usr/bin/env bash
set -euo pipefail
if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
cd "$(dirname "$0")/../.."
DEVDESK_SKIP_CREDENTIAL_HELPER_INSTALL=1 ./scripts/dev/build.sh
package_name="devdesk-0.2.0-$(rustc -vV | sed -n 's/^host: //p')"
staging="$(mktemp -d "${TMPDIR:-/tmp}/devdesk-package.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/$package_name" dist
cp target/release/devdesk-host "$staging/$package_name/"
cp docs/run-binary.md "$staging/$package_name/README.md"
cp -R docs "$staging/$package_name/"
cp -R containers "$staging/$package_name/"
cp -R permissions "$staging/$package_name/"
cp -R schemas "$staging/$package_name/"
mkdir -p "$staging/$package_name/scripts/upgrade"
cp scripts/upgrade/harness-v5.py "$staging/$package_name/scripts/upgrade/"
if [ "$(uname -s)" = Linux ]; then
  mkdir -p "$staging/$package_name/scripts/platform/linux"
  cp scripts/platform/linux/install-linux-sandbox-policy.sh "$staging/$package_name/scripts/platform/linux/"
fi
if [ "$(uname -s)" = Darwin ]; then
  ./scripts/platform/macos/install-macos-credential-helper.sh --bundle "$staging/$package_name/credential-helper"
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
