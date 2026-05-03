#!/usr/bin/env bash
# Builds a host-only XCFramework for `swift test` iteration on macOS.
#
# Unlike `build-xcframework.sh` (which targets every iOS + macOS architecture
# tuple and is intended for releases), this script only builds for the current
# host architecture. It exists so contributors can run the YSwift test target
# against in-development Rust changes without first rebuilding for every
# Apple platform.
#
# Usage:
#   ./scripts/build-xcframework-host.sh
#   YSWIFT_LOCAL=1 swift test
#
# The resulting XCFramework lands at `lib/yniffiFFI.xcframework` — same path
# the `YSWIFT_LOCAL` mode in `Package.swift` expects.

set -e
set -x

THIS_SCRIPT_DIR="$( cd -- "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"
pushd "$THIS_SCRIPT_DIR/../lib"

PACKAGE_NAME="yniffi"
LIB_NAME="libuniffi_yniffi.a"
FRAMEWORK_NAME="yniffiFFI"
SWIFT_FOLDER="swift"
BUILD_FOLDER="target"
XCFRAMEWORK_FOLDER="${FRAMEWORK_NAME}.xcframework"

HOST_TRIPLE="$(rustc -vV | sed -n 's|host: ||p')"

echo "▸ Cleaning previous host XCFramework"
rm -rf "${XCFRAMEWORK_FOLDER}"

echo "▸ Regenerating Swift scaffolding (UDL → yniffi.swift)"
mkdir -p "${SWIFT_FOLDER}/scaffold"
cargo run --manifest-path ./Cargo.toml \
    --features=uniffi/cli \
    --bin uniffi-bindgen generate \
    ./src/yniffi.udl \
    --language swift \
    --out-dir "${SWIFT_FOLDER}/scaffold"

echo "▸ Building static library for host (${HOST_TRIPLE})"
cargo build --target "${HOST_TRIPLE}" --package "${PACKAGE_NAME}" --release

echo "▸ Consolidating headers + modulemap"
mkdir -p "${BUILD_FOLDER}/includes"
cp "${SWIFT_FOLDER}/scaffold/yniffiFFI.h" "${BUILD_FOLDER}/includes"
cp "${SWIFT_FOLDER}/scaffold/yniffiFFI.modulemap" "${BUILD_FOLDER}/includes/module.modulemap"

xcodebuild -create-xcframework \
    -library "./${BUILD_FOLDER}/${HOST_TRIPLE}/release/${LIB_NAME}" \
    -headers "./${BUILD_FOLDER}/includes" \
    -output "./${XCFRAMEWORK_FOLDER}"

echo "▸ Done. Run YSWIFT_LOCAL=1 swift test"
