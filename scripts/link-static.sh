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
exec "${CC:-cc}" "${args[@]}"
