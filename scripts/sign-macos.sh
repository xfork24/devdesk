#!/usr/bin/env bash
set -euo pipefail
[ "$(uname -s)" = Darwin ] || exit 0
binary="${1:?provide the Host binary path}"
identity="${DEVDESK_CODESIGN_IDENTITY:-}"
identity_file="$HOME/Library/Application Support/DevDesk/signing/identity.txt"
if [ -z "$identity" ] && [ -f "$identity_file" ]; then IFS= read -r identity < "$identity_file"; fi
if [ -z "$identity" ]; then
  echo 'No stable DevDesk signing identity configured; see docs/system-credentials.md.'
  exit 0
fi
/usr/bin/codesign --force --sign "$identity" --identifier "${2:-dev.devdesk.host}" --timestamp=none "$binary"
/usr/bin/codesign --verify --strict "$binary"
