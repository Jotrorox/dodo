#!/usr/bin/env bash
# GNU/Linux C linker adapter for the release-small-static recipe.
set -euo pipefail

# llvm-sys declares its support libraries as dylibs, even with crt-static.
# Keep every library lookup static, including those emitted after Rust's CRT.
args=()
for arg in "$@"; do
    case "$arg" in
        -Wl,-Bdynamic) args+=(-Wl,-Bstatic) ;;
        *) args+=("$arg") ;;
    esac
done
# Rust emits the C runtime before llvm-sys's trailing libraries (libstdc++
# among them), and -nodefaultlibs drops the driver's own copy. Resolve their
# glibc and libgcc references (gettext, outline atomics) after the fact.
args+=(-lc -lgcc_eh -lgcc -lc)
exec "${CC:-cc}" "${args[@]}"
