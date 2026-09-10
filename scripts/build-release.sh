#!/usr/bin/env bash
# Build the self-contained Linux compiler. LLVM is a build-time dependency only.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

target=x86_64-unknown-linux-gnu
host=$(rustc -vV | sed -n 's/^host: //p')
if [[ "$host" != "$target" ]]; then
    echo "The self-contained release build currently supports native $target hosts." >&2
    echo "Use cargo build --locked --release for other hosts (LLVM is still static)." >&2
    exit 1
fi

# An explicit target keeps these flags off host build scripts and proc macros.
# LLVM itself is static through Cargo.toml; bundle its support libraries too.
# Keep the platform C runtime dynamic so the binary uses the host's libc.
flags=()
for library in z zstd stdc++ ffi; do
    archive=$("${CC:-cc}" "-print-file-name=lib${library}.a")
    if [[ ! -f "$archive" ]]; then
        echo "Missing static archive lib${library}.a; install the build prerequisites in README.md." >&2
        exit 1
    fi
    flags+=(-L "native=$(dirname "$archive")" -l "static=$library")
done

# Preserve caller flags, including paths with spaces in Cargo's encoded form.
if [[ ! -v CARGO_ENCODED_RUSTFLAGS ]]; then
    CARGO_ENCODED_RUSTFLAGS=$(python3 -c 'import os; print("\x1f".join(os.environ.get("RUSTFLAGS", "").split()))')
fi
for flag in "${flags[@]}"; do
    CARGO_ENCODED_RUSTFLAGS+="${CARGO_ENCODED_RUSTFLAGS:+$'\x1f'}$flag"
done
export CARGO_ENCODED_RUSTFLAGS

cargo build --locked --release --bin dodo --target "$target"
target_directory=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
binary="$target_directory/$target/release/dodo"
python3 scripts/check-linkage.py "$binary" --release
"$binary" --version
echo "Self-contained compiler: $binary"
