#!/bin/sh
# Compare the Dodo Lua compiler with the reference luac on Lua files.
#   tests/lua_compiler/compare.sh LUAC_DIR file.lua...
# For every file: the listings (`luac -l -l`, addresses normalized) and the
# binary chunks (`luac -o`) must be identical. Prints one line per file.
luacdir=$1; shift
root=$(cd "$(dirname "$0")/../.." && pwd)
tool="$root/target/luatool/luatool"
tmp=${TMPDIR:-/tmp}/luacmp.$$
mkdir -p "$tmp"
pass=0; fail=0
for f in "$@"; do
  "$luacdir/luac" -l -l -o "$tmp/ref.luac" "$f" > "$tmp/ref.txt" 2> "$tmp/ref.err"
  rs=$?
  "$tool" -l -o "$tmp/mine.luac" "$f" > "$tmp/mine.txt" 2> "$tmp/mine.err"
  ms=$?
  if [ $rs -ne 0 ] || [ $ms -ne 0 ]; then
    sed 's/^[^:]*luac: //; s/^luatool: //' "$tmp/ref.err" > "$tmp/ref.e"
    sed 's/^[^:]*luac: //; s/^luatool: //' "$tmp/mine.err" > "$tmp/mine.e"
    if [ $rs -ne 0 ] && [ $ms -ne 0 ] && cmp -s "$tmp/ref.e" "$tmp/mine.e"; then
      echo "SAME-ERROR $f"; pass=$((pass+1))
    else
      echo "ERROR-DIFF $f"; echo "  ref:  $(cat "$tmp/ref.e")"; echo "  mine: $(cat "$tmp/mine.e")"; fail=$((fail+1))
    fi
    continue
  fi
  sed 's/0x[0-9a-f]*/ADDR/g' "$tmp/ref.txt" > "$tmp/ref.n"
  sed 's/0x[0-9a-f]*/ADDR/g' "$tmp/mine.txt" > "$tmp/mine.n"
  if cmp -s "$tmp/ref.n" "$tmp/mine.n" && cmp -s "$tmp/ref.luac" "$tmp/mine.luac"; then
    echo "OK $f"; pass=$((pass+1))
  else
    lines=$(diff "$tmp/ref.n" "$tmp/mine.n" | grep -c '^[<>]')
    bin=same; cmp -s "$tmp/ref.luac" "$tmp/mine.luac" || bin=differs
    echo "DIFF $f ($lines listing lines differ, binary $bin)"; fail=$((fail+1))
  fi
done
rm -rf "$tmp"
echo "passed $pass, failed $fail"
