#!/usr/bin/env bash
# One local development identity; never imports or modifies unrelated identities.
set -euo pipefail
[ "$(uname -s)" = Darwin ] || { echo 'This setup is macOS-only.' >&2; exit 1; }
umask 077
signing_dir="$HOME/Library/Application Support/DevDesk/signing"
mkdir -p "$signing_dir"
chmod 700 "$signing_dir"
certificate="$signing_dir/devdesk-local.cert.pem"
private_key="$signing_dir/devdesk-local.key.pem"
if [ ! -f "$certificate" ] && [ ! -f "$private_key" ]; then
  cat > "$signing_dir/certificate.cnf" <<'EOF'
[req]
prompt=no
distinguished_name=subject
x509_extensions=extensions
[subject]
CN=DevDesk Local Development
O=DevDesk Local Development
[extensions]
basicConstraints=critical,CA:true
keyUsage=critical,digitalSignature,keyCertSign
extendedKeyUsage=critical,codeSigning
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid:always
EOF
  /usr/bin/openssl req -new -newkey rsa:2048 -nodes -x509 -sha256 -days 3650 -config "$signing_dir/certificate.cnf" -keyout "$private_key" -out "$certificate" 2>/dev/null
fi
[ -f "$certificate" ] && [ -f "$private_key" ] || { echo 'Incomplete signing files; refusing to overwrite.' >&2; exit 1; }
identity="$(/usr/bin/openssl x509 -in "$certificate" -noout -fingerprint -sha1 | sed 's/.*=//;s/://g')"
printf '%s\n' "$identity" > "$signing_dir/candidate-identity.txt"
if [ "${1:---prepare}" = --import ]; then
  login_keychain="$HOME/Library/Keychains/login.keychain-db"
  if /usr/bin/security find-identity -v -p codesigning | /usr/bin/grep -F "$identity" >/dev/null; then
    printf '%s\n' "$identity" > "$signing_dir/identity.txt"
    echo 'The local DevDesk signing identity is already configured.'
    exit 0
  fi
  # The private key is usable by codesign, not by every application.
  /usr/bin/security import "$private_key" -k "$login_keychain" -T /usr/bin/codesign
  /usr/bin/security add-certificates -k "$login_keychain" "$certificate"
  # User-domain trust is restricted to code signing; this is not a TLS root grant.
  /usr/bin/security add-trusted-cert -r trustRoot -p codeSign -k "$login_keychain" "$certificate"
  /usr/bin/security find-identity -v -p codesigning | /usr/bin/grep -F "$identity" >/dev/null
  printf '%s\n' "$identity" > "$signing_dir/identity.txt"
  echo 'Configured the local DevDesk signing identity.'
else
  printf 'Prepared certificate: %s\nPrepared private key: %s\n' "$certificate" "$private_key"
  echo 'Run scripts/setup-macos-signing.sh --import to import and trust it for code signing only.'
fi
