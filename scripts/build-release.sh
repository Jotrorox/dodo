#!/usr/bin/env bash
# Build the self-contained Linux compiler. LLVM is a build-time dependency only.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

profile=${1:-release}
if [[ $# -gt 1 || ( "$profile" != release && "$profile" != release-small && "$profile" != release-small-static ) ]]; then
    echo "Usage: $0 [release|release-small|release-small-static]" >&2
    exit 1
fi

target=x86_64-unknown-linux-gnu
host=$(rustc -vV | sed -n 's/^host: //p')
if [[ "$host" != "$target" ]]; then
    echo "The self-contained release build currently supports native $target hosts." >&2
    echo "Use cargo build --locked --release for other hosts." >&2
    exit 1
fi

# An explicit target keeps these flags off host build scripts and proc macros.
# Explicitly request static LLVM below; bundle its support libraries too.
flags=()
libraries=(z zstd stdc++ ffi)
declare -A library_archives=()
linkage_mode=--release
if [[ "$profile" == release-small-static ]]; then
    linkage_mode=--static
    # Avoid PIE relocation overhead and include the platform C runtime.
    flags+=(-C target-feature=+crt-static -C relocation-model=static
            -C "linker=$PWD/scripts/link-static.sh")
    libraries+=(c m)

    # LLVM builds can have additional dependencies (for example libxml2).
    # The static linker adapter also handles llvm-sys's dynamic declarations.
    if [[ -n "${LLVM_SYS_221_PREFIX:-}" ]]; then
        llvm_config="$LLVM_SYS_221_PREFIX/bin/llvm-config"
    else
        llvm_config=$(command -v llvm-config-22 || command -v llvm-config22 || command -v llvm-config || true)
    fi
    if [[ ! -x "$llvm_config" ]]; then
        echo "Set LLVM_SYS_221_PREFIX to the LLVM 22 installation for the static build." >&2
        exit 1
    fi
    system_libraries=$("$llvm_config" --link-static --system-libs)
    read -r -a system_libraries <<< "$system_libraries"
    for flag in "${system_libraries[@]}"; do
        if [[ "$flag" == -l* && "$flag" != -l:* ]]; then
            libraries+=("${flag#-l}")
        elif [[ "$flag" == /* && "${flag##*/}" =~ ^lib(.+)\.(a|so(\.[0-9.]+)?)$ ]]; then
            # Distribution LLVM packages can report absolute shared-library
            # paths even with --link-static. Resolve their static counterpart.
            library=${BASH_REMATCH[1]}
            libraries+=("$library")
            sibling="$(dirname "$flag")/lib${library}.a"
            if [[ -f "$sibling" ]]; then
                library_archives[$library]=$sibling
            fi
        else
            echo "Unsupported LLVM system-library flag for static linking: $flag" >&2
            exit 1
        fi
    done
fi
declare -A seen_libraries=()
for library in "${libraries[@]}"; do
    if [[ -v "seen_libraries[$library]" ]]; then
        continue
    fi
    seen_libraries[$library]=1
    archive=${library_archives[$library]:-}
    if [[ -z "$archive" ]]; then
        archive=$("${CC:-cc}" "-print-file-name=lib${library}.a")
    fi
    if [[ ! -f "$archive" ]]; then
        echo "Missing static archive lib${library}.a; install the build prerequisites in docs/src/content/docs/building-from-source.md." >&2
        exit 1
    fi
    flags+=(-L "native=$(dirname "$archive")")
    if [[ "$profile" != release-small-static ]]; then
        flags+=(-l "static=$library")
    fi
done

# Preserve caller flags, including paths with spaces in Cargo's encoded form.
if [[ ! -v CARGO_ENCODED_RUSTFLAGS ]]; then
    CARGO_ENCODED_RUSTFLAGS=$(python3 -c 'import os; print("\x1f".join(os.environ.get("RUSTFLAGS", "").split()))')
fi
for flag in "${flags[@]}"; do
    CARGO_ENCODED_RUSTFLAGS+="${CARGO_ENCODED_RUSTFLAGS:+$'\x1f'}$flag"
done
export CARGO_ENCODED_RUSTFLAGS

cargo build --locked --features llvm-sys/force-static --profile "$profile" --bin dodo --target "$target"
target_directory=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
binary="$target_directory/$target/$profile/dodo"
python3 scripts/check-linkage.py "$binary" "$linkage_mode"
"$binary" --version
echo "Self-contained compiler: $binary"
