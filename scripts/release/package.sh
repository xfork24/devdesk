#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

usage() {
  printf 'Usage: %s host [--profile size|speed|symbols]\n' "$0"
  printf '       %s web|software-development|sdk\n' "$0"
}

target="${1:-}"
profile=release
suffix=
case "$target" in
  --help|-h)
    usage
    exit 0
    ;;
  host)
    if [[ $# == 3 && "$2" == --profile ]]; then
      case "$3" in
        size) ;;
        speed|symbols)
          profile="release-$3"
          suffix="-$3"
          ;;
        *)
          usage >&2
          exit 2
          ;;
      esac
    elif [[ $# != 1 ]]; then
      usage >&2
      exit 2
    fi
    ;;
  web|software-development|sdk)
    if [[ $# != 1 ]]; then
      usage >&2
      exit 2
    fi
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

output="${DEVDESK_RELEASE_OUT:-dist}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
stage="$(mktemp -d "${TMPDIR:-/tmp}/devdesk-core.XXXXXX")"
trap 'rm -rf "$stage"' EXIT

case "$target" in
  host)
    if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
    cargo build --profile "$profile" --locked -p devdesk-host
    binary="${CARGO_TARGET_DIR:-target}/$profile/devdesk-host"
    ./scripts/platform/macos/sign-macos.sh "$binary"
    version=$("$binary" --version)
    name="devdesk-core-host-${version##* }-$(rustc -vV | sed -n 's/^host: //p')$suffix"
    mkdir "$stage/$name"
    cp "$binary" "$stage/$name/"
    cp permissions/backends/docker/upstream/LICENSE.moby "$stage/$name/"
    artifact="$name.tar.gz"
    COPYFILE_DISABLE=1 tar -czf "$stage/$artifact" -C "$stage" "$name"
    ;;
  web)
    npm --prefix web ci
    npm --prefix web run build:core
    web/node_modules/.bin/tsc -p web/tsconfig.sdk.json
    version=$(node -p 'require("./web/package.json").version')
    name="devdesk-core-web-$version"
    mkdir "$stage/$name"
    cp -R web/dist/core/. "$stage/$name/"
    cp schemas/domain-{package,contributions,entity-types}.schema.json "$stage/$name/sdk/"
    artifact="$name.tar.gz"
    tar -czf "$stage/$artifact" -C "$stage" "$name"
    ;;
  software-development)
    npm --prefix web ci
    npm --prefix web run build:software
    artifact="software-development-$(python3 plugins/software-development/build.py --version).zip"
    python3 plugins/software-development/build.py "$stage/$artifact"
    ;;
  sdk)
    if ! command -v cargo >/dev/null && [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
    cargo build --locked -p devdesk-host
    host="${CARGO_TARGET_DIR:-target}/debug/devdesk-host"
    ./scripts/platform/macos/sign-macos.sh "$host"
    version=$(node -p 'require("./sdk/domain/package.json").version')
    name="devdesk-domain-sdk-$version"
    mkdir -p "$stage/$name/schemas" "$stage/$name/kernel"
    cp -R sdk/domain/cli sdk/domain/lib sdk/domain/worker sdk/domain/package.json "$stage/$name/"
    cp schemas/domain-package.schema.json schemas/domain-contributions.schema.json \
       schemas/domain-entity-types.schema.json schemas/template-manifest.schema.json \
       schemas/source-definition.schema.json "$stage/$name/schemas/"
    cp -R kernel/core kernel/engineering "$stage/$name/kernel/"
    "$host" capabilities --print > "$stage/$name/catalog.json"
    artifact="$name.tar.gz"
    COPYFILE_DISABLE=1 tar -czf "$stage/$artifact" -C "$stage" "$name"
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
