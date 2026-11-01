#!/bin/sh
# End-to-end check on the reference interpreter: run every file of the
# official Lua test suite three ways -- from source, as a luac-compiled
# chunk, and as a chunk compiled by the Dodo compiler -- and report the
# outcome of each.
#   tests/lua_compiler/run_suite.sh LUA_SRC_DIR TESTS_DIR
luadir=$1; tests=$2
root=$(cd "$(dirname "$0")/../.." && pwd)
tool="$root/target/luatool/luatool"
work=${TMPDIR:-/tmp}/luasuite.$$
for mode in src luac dodo; do
  rm -rf "$work/$mode"; mkdir -p "$work/$mode"; cp -R "$tests"/. "$work/$mode/"
done
for f in "$tests"/*.lua; do
  b=$(basename "$f")
  "$luadir/luac" -o "$work/luac/$b" "$f"
  "$tool" -o "$work/dodo/$b" "$f"
done
for f in "$tests"/*.lua; do
  b=$(basename "$f")
  # all.lua drives the whole suite; heavy.lua deliberately exhausts memory.
  [ "$b" = all.lua ] && continue
  [ "$b" = heavy.lua ] && continue
  line="$b"
  for mode in src luac dodo; do
    (cd "$work/$mode" && "$luadir/lua" -e "_U=true" "$b" > "$work/$mode.$b.out" 2>&1)
    st=$?
    if [ $st -eq 0 ]; then r=pass; else r="FAIL($(grep -m1 -o '[a-z]*\.lua:[0-9]*:.*' "$work/$mode.$b.out" | head -c 60))"; fi
    line="$line | $mode: $r"
  done
  echo "$line"
done
rm -rf "$work"
