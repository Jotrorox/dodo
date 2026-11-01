#!/bin/sh
# Build the Lua compiler test tool: the stdlib package sources plus
# tests/lua_compiler/tool/lua_tool.dodo are assembled into one package in
# target/luatool and compiled with the freshly built dodo.
#   tests/lua_compiler/build_tool.sh [-O2]
set -e
root=$(cd "$(dirname "$0")/../.." && pwd)
out="$root/target/luatool"
rm -rf "$out/lua"
mkdir -p "$out/lua"
cp "$root"/stdlib/std/script/lua/*.dodo "$out/lua/"
cp "$root/tests/lua_compiler/tool/lua_tool.dodo" "$out/lua/"
cp "$root/tests/lua_compiler/tool/main.dodo" "$out/main.dodo"
# Build next to the old binary and swap, so running comparisons keep working.
"$root/target/debug/dodo" build "$@" "$out/main.dodo" -o "$out/luatool.new"
mv "$out/luatool.new" "$out/luatool"
