#!/bin/sh
# Regenerate the expected outputs of tests/lua_compiler.rs with a reference
# PUC Lua 5.4 luac:   tests/lua_compiler/update_expected.sh LUAC_DIR
# Listings have their addresses replaced by ADDR; error files hold the
# message without the "luac: " prefix.
luacdir=$1
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/snippets"
for f in *.lua; do
  "$luacdir/luac" -l -l -p "$f" | sed 's/0x[0-9a-f]*/ADDR/g' > "${f%.lua}.expected"
done
cd "$here/errors"
for f in *.lua; do
  "$luacdir/luac" -p "$f" 2>&1 | sed 's/^.*luac: //' > "${f%.lua}.expected"
done
