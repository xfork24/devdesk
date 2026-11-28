#!/bin/bash
# Public DSH runnerCommand seam. Docker already owns the outer PID namespace.
# Keep a private tool PID namespace, but use an empty /proc: Docker's default
# masked proc submounts intentionally prevent mounting another proc filesystem.
# No parent environment, harness state or coordinator policy is exposed to tools.
set -euo pipefail
network=(--unshare-net)
# DevDesk supplies this prefix from the frozen permission profile.
if [[ ${1:-} == --allow-network ]]; then
  network=()
  shift
fi
profile=()
while (($#)) && [[ "$1" != -- ]]; do
  if [[ "$1" == --proc ]]; then
    [[ $# -ge 2 && "$2" == /proc ]] || exit 64
    profile+=(--tmpfs /proc)
    shift 2
  else
    profile+=("$1")
    shift
  fi
done
[[ $# -gt 1 && "$1" == -- ]] || exit 64
shift
exec /usr/bin/bwrap "${network[@]}" "${profile[@]}" --tmpfs /state --tmpfs /run/devdesk -- "$@"
