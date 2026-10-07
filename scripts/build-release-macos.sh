#!/usr/bin/env bash
# Build the self-contained macOS compiler. LLVM is a build-time dependency only.
# Compatible with the bash 3.2 that macOS ships.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $# -ne 0 ]]; then
    echo "Usage: $0" >&2
    exit 1
fi

host=$(rustc -vV | sed -n 's/^host: //p')
case "$host" in
    aarch64-apple-darwin | x86_64-apple-darwin) target=$host ;;
    *)
        echo "The macOS release build supports native aarch64 and x86_64 macOS hosts." >&2
        exit 1
        ;;
esac

# Homebrew's llvm@23 provides native static libraries. (The official macOS
# release archive ships ThinLTO bitcode, which Rust's own LLVM cannot link.)
if [[ -z "${LLVM_SYS_231_PREFIX:-}" ]] && command -v brew > /dev/null; then
    LLVM_SYS_231_PREFIX=$(brew --prefix llvm@23)
    export LLVM_SYS_231_PREFIX
fi
if [[ -z "${LLVM_SYS_231_PREFIX:-}" || ! -x "$LLVM_SYS_231_PREFIX/bin/llvm-config" ]]; then
    echo "Set LLVM_SYS_231_PREFIX to an LLVM 23 installation (brew install llvm@23)." >&2
    exit 1
fi

# Releases run on the oldest macOS they were linked for. Static archives from
# the build machine (for example Homebrew's zstd) must not be newer.
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-15.0}

# A release may only need libraries that every macOS installation provides.
# LLVM support libraries outside the SDK are linked from their static archives:
# a directory holding only those archives precedes the other search paths, and
# ld64 checks each directory for a dylib or an archive before the next one.
static_directory=$(mktemp -d "${TMPDIR:-/tmp}/dodo-static.XXXXXX")
trap 'rm -rf "$static_directory"' EXIT
search_directories=()
if [[ -n "${LIBRARY_PATH:-}" ]]; then
    IFS=: read -r -a search_directories <<< "$LIBRARY_PATH"
fi
if command -v brew > /dev/null; then
    search_directories+=("$(brew --prefix)/lib")
fi
search_directories+=(/opt/homebrew/lib /usr/local/lib)

system_libraries=$("$LLVM_SYS_231_PREFIX/bin/llvm-config" --link-static --system-libs)
for flag in $system_libraries; do
    case "$flag" in
        # Provided by macOS itself.
        -lz | -lm | -lxml2 | -liconv | -lc++ | -lpthread | -ldl | -lncurses | -ltinfo) ;;
        -l* | /*.a)
            # Official LLVM archives name some dependencies by the absolute
            # path of the build machine's static archive.
            library=${flag#-l}
            if [[ "$flag" == /* ]]; then
                library=$(basename "$flag" .a)
                library=${library#lib}
            fi
            archive=
            if [[ "$flag" == /* && -f "$flag" ]]; then
                archive=$flag
            fi
            for directory in ${search_directories[@]+"${search_directories[@]}"}; do
                if [[ -z "$archive" && -f "$directory/lib$library.a" ]]; then
                    archive="$directory/lib$library.a"
                    break
                fi
            done
            if [[ -z "$archive" ]]; then
                echo "Missing static archive lib$library.a; install it (for example brew install $library) or add its directory to LIBRARY_PATH." >&2
                exit 1
            fi
            ln -s "$archive" "$static_directory/lib$library.a"
            ;;
        /*.dylib)
            # Optional LLVM features Dodo never calls, such as Homebrew's Z3
            # solver. -dead_strip_dylibs drops the unused reference, and the
            # linkage check below fails the build if it remains.
            echo "Expecting the linker to drop unused $flag" >&2
            ;;
        *)
            echo "Unsupported LLVM system-library flag for a macOS release: $flag" >&2
            exit 1
            ;;
    esac
done

# Preserve caller flags, including paths with spaces in Cargo's encoded form.
if [[ -z "${CARGO_ENCODED_RUSTFLAGS+set}" ]]; then
    CARGO_ENCODED_RUSTFLAGS=$(python3 -c 'import os; print("\x1f".join(os.environ.get("RUSTFLAGS", "").split()))')
fi
CARGO_ENCODED_RUSTFLAGS+="${CARGO_ENCODED_RUSTFLAGS:+$'\x1f'}-L"$'\x1f'"native=$static_directory"
CARGO_ENCODED_RUSTFLAGS+=$'\x1f'"-C"$'\x1f'"link-arg=-Wl,-dead_strip_dylibs"
export CARGO_ENCODED_RUSTFLAGS

cargo build --locked --features llvm-sys/force-static --release --bin dodo --target "$target"
target_directory=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
binary="$target_directory/$target/release/dodo"
python3 scripts/check-linkage.py "$binary" --release
"$binary" --version
echo "Self-contained compiler: $binary"
