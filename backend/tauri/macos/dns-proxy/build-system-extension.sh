#!/bin/sh
set -eu

script_dir="$(cd "$(dirname "$0")" && pwd)"
derived_data_dir="$script_dir/.build/DerivedData"
products_dir="$script_dir/.build/products"
product_path="$products_dir/chimera-dns-proxy.systemextension"
development_team="${CHIMERA_DEVELOPMENT_TEAM:-}"

if [ "$(uname -s)" != "Darwin" ]; then
	printf '%s\n' "Skipping the macOS DNS Proxy extension build on a non-macOS host."
	exit 0
fi

command -v xcodebuild >/dev/null 2>&1 || {
	printf '%s\n' "xcodebuild is required to build the macOS DNS Proxy extension." >&2
	exit 1
}

xcodebuild \
	-project "$script_dir/ChimeraDNSProxy.xcodeproj" \
	-scheme ChimeraDNSProxyExtension \
	-configuration Release \
	-sdk macosx \
	-derivedDataPath "$derived_data_dir" \
	CONFIGURATION_BUILD_DIR="$products_dir" \
	DEVELOPMENT_TEAM="$development_team" \
	CODE_SIGNING_ALLOWED=NO \
	ONLY_ACTIVE_ARCH=NO \
	-arch arm64 \
	-arch x86_64 \
	-quiet \
	build

test -x "$product_path/Contents/MacOS/chimera-dns-proxy"
plutil -lint "$product_path/Contents/Info.plist"
printf 'Built unsigned universal extension: %s\n' "$product_path"
