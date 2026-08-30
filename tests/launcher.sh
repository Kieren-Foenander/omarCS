#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
test_binary="${OMARCS_TEST_BINARY:-$repo_dir/target/release/omarcs}"
test_root="$(mktemp -d)"
trap 'rm -rf -- "$test_root"' EXIT

test -x "$test_binary"
mkdir -p "$test_root/assets" "$test_root/mock-bin"
install -m 0755 -- "$test_binary" "$test_root/assets/omarcs"

asset="omarcs-x86_64-unknown-linux-musl.tar.gz"
tar -czf "$test_root/assets/$asset" -C "$test_root/assets" omarcs
(
  cd "$test_root/assets"
  sha256sum "$asset" > "$asset.sha256"
)

cat > "$test_root/mock-bin/curl" <<'MOCK_CURL'
#!/usr/bin/env bash
set -euo pipefail

output=""
url=""
while (($#)); do
  if [[ "$1" == "--output" ]]; then
    output="$2"
    shift 2
  else
    url="$1"
    shift
  fi
done

test -n "$output"
source_name="${url##*/}"
cp -- "$OMARCS_TEST_ASSET_DIR/$source_name" "$output"
printf '%s\n' "$url" >> "$OMARCS_TEST_CURL_LOG"
MOCK_CURL

cat > "$test_root/mock-bin/cargo" <<'MOCK_CARGO'
#!/usr/bin/env bash
echo "launcher unexpectedly fell back to Cargo" >&2
exit 99
MOCK_CARGO
chmod 0755 "$test_root/mock-bin/curl" "$test_root/mock-bin/cargo"

export PATH="$test_root/mock-bin:$PATH"
export XDG_DATA_HOME="$test_root/data"
export XDG_CACHE_HOME="$test_root/cache"
export OMARCS_RELEASE_BASE_URL="https://release.test.invalid"
export OMARCS_TEST_ASSET_DIR="$test_root/assets"
export OMARCS_TEST_CURL_LOG="$test_root/curl.log"

"$repo_dir/omarcs-plugin" --help >/dev/null
test -x "$XDG_DATA_HOME/omarcs/omarcs"
test "$(tr -d '\n' < "$XDG_DATA_HOME/omarcs/omarcs.version")" = \
  "release:v0.1.1:x86_64-unknown-linux-musl"
test "$(wc -l < "$OMARCS_TEST_CURL_LOG")" -eq 2
test -z "$(find "$XDG_CACHE_HOME/omarcs" -mindepth 1 -print -quit)"

# A second launch must use the installed version without touching the network.
"$repo_dir/omarcs-plugin" --help >/dev/null
test "$(wc -l < "$OMARCS_TEST_CURL_LOG")" -eq 2

# A failed release download falls back to Cargo, records the source install,
# and removes its temporary build directory. The mock keeps this test fast;
# normal workspace verification separately performs the real release build.
cat > "$test_root/mock-bin/cargo" <<'MOCK_CARGO'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$CARGO_TARGET_DIR/release"
cp -- "$OMARCS_TEST_BINARY" "$CARGO_TARGET_DIR/release/omarcs"
printf 'cargo-called\n' >> "$OMARCS_TEST_CARGO_LOG"
MOCK_CARGO
chmod 0755 "$test_root/mock-bin/cargo"

export XDG_DATA_HOME="$test_root/source-data"
export XDG_CACHE_HOME="$test_root/source-cache"
export OMARCS_TEST_ASSET_DIR="$test_root/missing-assets"
export OMARCS_TEST_BINARY="$test_binary"
export OMARCS_TEST_CARGO_LOG="$test_root/cargo.log"

if ! "$repo_dir/omarcs-plugin" --help >/dev/null 2>"$test_root/fallback.log"; then
  cat "$test_root/fallback.log" >&2
  exit 1
fi
test -x "$XDG_DATA_HOME/omarcs/omarcs"
test "$(tr -d '\n' < "$XDG_DATA_HOME/omarcs/omarcs.version")" = \
  "source:v0.1.1:$(uname -m)"
test "$(wc -l < "$OMARCS_TEST_CARGO_LOG")" -eq 1
test -z "$(find "$XDG_CACHE_HOME/omarcs" -mindepth 1 -print -quit)"

# Installed plugin clones may contain the old in-repository Cargo build cache.
# It is generated data and should disappear after the standalone binary exists.
installed_plugin="$test_root/config/omarchy/plugins/omarcs.stats"
mkdir -p "$installed_plugin/target"
install -m 0755 -- "$repo_dir/omarcs-plugin" "$installed_plugin/omarcs-plugin"
install -m 0644 -- "$repo_dir/omarcs-release" "$installed_plugin/omarcs-release"
touch "$installed_plugin/target/legacy-build-artifact"

export XDG_CONFIG_HOME="$test_root/config"
export XDG_DATA_HOME="$test_root/plugin-data"
export XDG_CACHE_HOME="$test_root/plugin-cache"
export OMARCS_TEST_ASSET_DIR="$test_root/assets"
"$installed_plugin/omarcs-plugin" --help >/dev/null
test ! -e "$installed_plugin/target"
