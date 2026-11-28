#!/bin/sh
set -eu

commit="$1"
expected="$2"
destination="$3"

case "$(uname -m)" in
  aarch64) node_package=node-linux-arm64 ;;
  x86_64) node_package=node-linux-x64 ;;
  *) echo 'Unsupported ZCode CLI build architecture' >&2; exit 1 ;;
esac

mkdir -p "$destination/workspace" "$destination/toolchain"
archive=$(mktemp)
trap 'rm -f "$archive"' EXIT HUP INT TERM
if [ -n "${DEVDESK_ZCODE_SOURCE_ARCHIVE:-}" ]; then
  cp "$DEVDESK_ZCODE_SOURCE_ARCHIVE" "$archive"
else
  curl --fail --silent --show-error --location --retry 2 --max-time 300 \
    "https://codeload.github.com/zai-org/ZCode/tar.gz/$commit" -o "$archive"
fi
printf '%s  %s\n' "$expected" "$archive" | sha256sum --check
tar -xzf "$archive" --strip-components=1 -C "$destination/workspace"

npm install --prefix "$destination/toolchain" --ignore-scripts --no-audit --no-fund \
  "$node_package@24.14.0" pnpm@10.33.2
node="$destination/toolchain/node_modules/$node_package/bin/node"
pnpm="$destination/toolchain/node_modules/pnpm/bin/pnpm.cjs"
export PATH="$(dirname "$node"):$PATH"
export CI=1 HUSKY=0
cd "$destination/workspace"
"$node" "$pnpm" --filter zcode-cli --filter @zcode/cli... install --frozen-lockfile --ignore-scripts
"$node" "$pnpm" --filter @zcode/cli... build

rm -f apps/zcode-cli/packages/cli/dist/zcode.cjs.map
cat > "$destination/zcode" <<'SH'
#!/bin/sh
set -eu
runtime=$(dirname "$(readlink -f "$0")")
case "$(uname -m)" in
  aarch64) node_package=node-linux-arm64 ;;
  x86_64) node_package=node-linux-x64 ;;
  *) exit 1 ;;
esac
exec "$runtime/toolchain/node_modules/$node_package/bin/node" \
  "$runtime/workspace/apps/zcode-cli/packages/cli/dist/zcode.cjs" "$@"
SH
chmod 0755 "$destination/zcode"
