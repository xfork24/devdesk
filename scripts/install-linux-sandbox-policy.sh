#!/usr/bin/env bash
# Install only DevDesk's named worker profile; never change docker-default,
# daemon settings, sysctls, capabilities or AppArmor's enforcement mode.
set -euo pipefail
cd "$(dirname "$0")/.."
policy=permissions/backends/docker/apparmor/devdesk-harness-v1.apparmor
destination=/etc/apparmor.d/devdesk-harness-v1
if [[ "$(uname -s)" != Linux ]]; then
  echo "This profile is for an AppArmor-enabled Linux Docker host." >&2
  exit 1
fi
apparmor_parser --skip-kernel-load --skip-cache "$policy"
if [[ "${1:---check}" == --check ]]; then
  sha256sum "$policy"
  exit 0
fi
if [[ "${1:-}" != --install || $# != 1 ]]; then
  echo "Usage: $0 --check | --install" >&2
  exit 2
fi
if [[ "$(id -u)" != 0 ]]; then
  echo "Loading a named kernel profile requires root. Run: sudo $0 --install" >&2
  exit 1
fi
if [[ -L "$destination" ]]; then
  echo "Refusing a symlink at $destination" >&2
  exit 1
fi
if [[ -e "$destination" ]] && ! cmp -s "$policy" "$destination"; then
  echo "Existing profile differs; review/version it explicitly before replacing it." >&2
  exit 1
fi
install -m 0644 -o root -g root "$policy" "$destination"
apparmor_parser --replace --skip-cache "$destination"
sha256sum "$destination"
echo "Loaded devdesk-harness-v1. docker-default and existing containers are unchanged."
