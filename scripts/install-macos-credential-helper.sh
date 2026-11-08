#!/usr/bin/env bash
# A shared immutable Keychain principal, independent of Host/project/role builds.
set -euo pipefail
[ "$(uname -s)" = Darwin ] || exit 0
cd "$(dirname "$0")/.."
umask 077
identity="${DEVDESK_CODESIGN_IDENTITY:-}"
identity_file="$HOME/Library/Application Support/DevDesk/signing/identity.txt"
if [ -z "$identity" ] && [ -f "$identity_file" ]; then IFS= read -r identity < "$identity_file"; fi
[ -n "$identity" ] || { echo 'Configure the stable signing identity first; see docs/system-credentials.md.' >&2; exit 1; }
if [[ "$identity" =~ ^[A-Fa-f0-9]{40}$ ]]; then
  certificate="$identity"
else
  certificate="$(/usr/bin/security find-certificate -c "$identity" -Z | sed -n 's/^ *SHA-1 hash: //p' | head -1)"
fi
if [ -n "${DEVDESK_CODESIGN_CERTIFICATE_SHA1:-}" ]; then certificate="$DEVDESK_CODESIGN_CERTIFICATE_SHA1"; fi
[[ "$certificate" =~ ^[A-Fa-f0-9]{40}$ ]] || { echo 'Set DEVDESK_CODESIGN_CERTIFICATE_SHA1 to the signing certificate fingerprint.' >&2; exit 1; }
export DEVDESK_ALLOWED_HOST_CERTIFICATE="$certificate"
helper_dir="$HOME/Library/Application Support/DevDesk/credential-helper/v1"
mkdir -p "$helper_dir"
chmod 700 "$helper_dir"
for attempt in {1..200}; do
  if mkdir "$helper_dir/.install-lock" 2>/dev/null; then break; fi
  [ "$attempt" -lt 200 ] || { echo 'Credential helper install is already running.' >&2; exit 1; }
  sleep .1
done
trap 'rmdir "$helper_dir/.install-lock"' EXIT
source_digest="$(python3 scripts/py/credential-helper-fingerprint.py "$certificate")"
if [ -f "$helper_dir/manifest.json" ] && [ -f "$helper_dir/devdesk-credential-helper" ]; then
  existing="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["source_digest"])' "$helper_dir/manifest.json")"
  if [ "$existing" = "$source_digest" ]; then
    python3 - "$helper_dir" <<'PYVERIFY'
import json,sys,pathlib,hashlib
root=pathlib.Path(sys.argv[1]);manifest=json.loads((root/'manifest.json').read_text());actual=hashlib.sha256((root/'devdesk-credential-helper').read_bytes()).hexdigest()
if actual!=manifest['binary_sha256']:raise SystemExit('Installed credential helper content changed; explicit repair is required.')
PYVERIFY
    /usr/bin/codesign --verify --strict -R "=identifier \"dev.devdesk.credentials\" and certificate leaf = H\"$certificate\"" "$helper_dir/devdesk-credential-helper"
    echo 'Reused the installed macOS credential helper.'
    exit 0
  fi
  [ "${1:-}" = --update ] || { echo 'Credential helper implementation changed; explicit --update is required and may require one new OS authorization.' >&2; exit 1; }
fi
cargo build --release --locked -p devdesk-credential-helper
./scripts/sign-macos.sh target/release/devdesk-credential-helper dev.devdesk.credentials
cp target/release/devdesk-credential-helper "$helper_dir/.helper-pending"
chmod 700 "$helper_dir/.helper-pending"
mv "$helper_dir/.helper-pending" "$helper_dir/devdesk-credential-helper"
python3 - "$helper_dir" "$source_digest" "$certificate" <<'PY'
import json,sys,pathlib,hashlib
root=pathlib.Path(sys.argv[1]);value={'protocol':1,'source_digest':sys.argv[2],'certificate_sha1':sys.argv[3],'binary_sha256':hashlib.sha256((root/'devdesk-credential-helper').read_bytes()).hexdigest()}
p=root/'.manifest-pending';p.write_text(json.dumps(value,indent=2)+'\n');p.replace(root/'manifest.json')
PY
echo 'Installed the shared macOS credential helper.'
