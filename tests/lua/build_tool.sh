#!/bin/sh
# Build the Lua compiler test tool: the stdlib package sources plus
# tests/lua/tool/lua_tool.dodo are assembled into one package in
# target/luatool and compiled with the freshly built dodo.
#   tests/lua/build_tool.sh [-O2]
set -e
root=$(cd "$(dirname "$0")/../.." && pwd)
out="$root/target/luatool"
rm -rf "$out"
mkdir -p "$out/lua"
cp "$root"/stdlib/std/script/lua/*.dodo "$out/lua/"
cp "$root/tests/lua/tool/lua_tool.dodo" "$out/lua/"
cp "$root/tests/lua/tool/main.dodo" "$out/main.dodo"
"$root/target/debug/dodo" build "$@" "$out/main.dodo" -o "$out/luatool"
