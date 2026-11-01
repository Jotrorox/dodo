# Lua compiler tests

Tests for the Lua 5.4 compiler in `stdlib/std/script/lua` (`lex.dodo`,
`parse.dodo`, `code.dodo`). The compiler must produce exactly what the
reference `luac` 5.4 produces: the same instructions, constants, debug
information (line info, locals, upvalues) and the same syntax errors.

## Pieces

- `tool/` -- a test driver. `lua_tool.dodo` is a `package lua` file that is
  compiled *together with* the stdlib package sources (it needs the
  package's internals); `build_tool.sh` assembles both into
  `target/luatool/` and builds `target/luatool/luatool`:

      luatool [-l] [-o out.luac] [-t N] file.lua

  `-l` prints a listing in the exact format of `luac -l -l`, `-o` writes a
  binary chunk in the Lua 5.4 format (loadable by the reference `lua`),
  `-t N` compiles N times and reports the time per compilation. Like
  `luaL_loadfile`, it skips a BOM and a first line starting with `#`.
- `snippets/*.lua` -- programs covering every construct; `*.expected` holds
  the reference listing (addresses replaced by `ADDR`).
- `errors/*.lua` -- syntax and semantic errors; `*.expected` holds the
  reference message (empty for the few valid programs among them).
- `../lua_compiler.rs` -- the `cargo test` that builds the tool and checks
  both directories against the expected files.

## Comparing with a reference Lua

Build PUC Lua 5.4.9 (`make macosx` or `make linux`) somewhere; `LUA_SRC_DIR` is
the directory holding its `lua` and `luac` (for example `lua-5.4.9/src`, or
`target/lua-reference/bin` once `scripts/test_lua.py fetch` exists). Then:

    tests/lua_compiler/build_tool.sh -O2
    tests/lua_compiler/compare.sh LUA_SRC_DIR files.lua...      # listings + binary chunks
    tests/lua_compiler/update_expected.sh LUA_SRC_DIR           # refresh *.expected
    python3 tests/lua_compiler/gen_cases.py OUT && tests/lua_compiler/compare.sh LUA_SRC_DIR OUT/*.lua
    python3 tests/lua_compiler/fuzz.py OUT 3000 SEED && tests/lua_compiler/compare.sh LUA_SRC_DIR OUT/*.lua
    tests/lua_compiler/run_suite.sh LUA_SRC_DIR LUA_TESTS_DIR   # run the official suite

`compare.sh` requires both the normalized `luac -l -l` listing and the
`luac -o` binary chunk to be byte-identical (or both compilers to fail with
the same message). `gen_cases.py` generates limit cases (locals, upvalues,
registers, constants past the RK and LOADK ranges, long loops, SETLIST and
NEWTABLE sizes, line-info edge cases, folding). `fuzz.py` generates random
programs. `run_suite.sh` runs each file of the official test suite from
source, as a luac chunk and as a chunk of this compiler.

Known, intended difference: when the parser's nesting limit is hit,
PUC Lua 5.4.9 reports `C stack overflow` (a C-stack error without
position); this compiler reports `chunk has too many syntax levels` at the
current line, and its nesting counter starts at 0 instead of the caller's
C-call depth (one level more than luac accepts).
