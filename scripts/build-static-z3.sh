#!/usr/bin/env bash
# Supply the static Z3 archive missing from Ubuntu's libz3-dev package.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
if [[ $# -gt 1 ]]; then
    echo "Usage: $0 [install-prefix]" >&2
    exit 1
fi

prefix=${1:-"$PWD/target/static-deps"}
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)
work=$(mktemp -d -t dodo-static-z3.XXXXXXXX)
trap 'rm -rf "$work"' EXIT

# Z3 4.8.12 matches Ubuntu 24.04's LLVM support-library API. Pin both the
# upstream commit and archive checksum so CI does not build a moving target.
revision=3a402ca2c14c3891d24658318406f80ce59b719f
checksum=247ce6c545e9e09890b3de0aa075e567999eb897e7122a4dc4d94e708b8fba1a
curl --fail --silent --show-error --location \
    --retry 5 --retry-all-errors --connect-timeout 30 --max-time 300 --retry-max-time 600 \
    "https://codeload.github.com/Z3Prover/z3/tar.gz/$revision" -o "$work/z3.tar.gz"
echo "$checksum  $work/z3.tar.gz" | sha256sum --check --status
tar -xzf "$work/z3.tar.gz" -C "$work"
cmake -S "$work/z3-$revision" -B "$work/build" -G Ninja \
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_INSTALL_LIBDIR=lib -DZ3_BUILD_LIBZ3_SHARED=OFF \
    -DZ3_BUILD_EXECUTABLE=OFF -DZ3_BUILD_TEST_EXECUTABLES=OFF \
    -DZ3_ENABLE_EXAMPLE_TARGETS=OFF -DZ3_BUILD_PYTHON_BINDINGS=OFF \
    -DZ3_USE_LIB_GMP=OFF -DZ3_INCLUDE_GIT_HASH=OFF -DZ3_INCLUDE_GIT_DESCRIBE=OFF
cmake --build "$work/build" --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-2}"
cmake --install "$work/build"
install -D -m 644 "$work/z3-$revision/LICENSE.txt" "$prefix/share/licenses/z3/LICENSE.txt"
test -s "$prefix/lib/libz3.a"
echo "Static Z3 archive: $prefix/lib/libz3.a"
echo "Add $prefix/lib to LIBRARY_PATH when running scripts/build-release.sh."
