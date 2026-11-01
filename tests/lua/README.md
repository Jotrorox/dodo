# Lua 5.4 conformance corpus

Tests for Dodo's Lua 5.4 implementation (`std/script/lua`) and its stand-alone
interpreter (`examples/lua`, built to `build/lua`). Every script here is an
original test that prints deterministic output. The reference behaviour is
PUC Lua 5.4.9, recorded in the matching `.expected` file, so CI can check our
interpreter without building the reference.

Everything is driven by `scripts/test_lua.py`, which needs Python 3.9+ and only
the standard library.

## Workflow

```sh
# Build our interpreter (-O 0 works too; it is just slower)
target/debug/dodo build examples/lua -O 2 -o build/lua

# Check it against the committed expected files (no reference needed)
python3 scripts/test_lua.py check                    # all of tests/lua
python3 scripts/test_lua.py check pattern_ strlib_   # name prefixes
python3 scripts/test_lua.py check --lua path/to/lua tests/lua/meta_gc.lua

# Download, verify (pinned sha256) and build the reference PUC Lua 5.4.9 and
# the official test suite into target/lua-reference/ (ignored by git)
python3 scripts/test_lua.py fetch

# Run both interpreters side by side and show unified diffs
python3 scripts/test_lua.py diff [--strict-stderr] [names...]

# After adding or changing a test, regenerate its expected file
python3 scripts/test_lua.py expect [names...]

# The official Lua 5.4.9 test suite, one file per process
python3 scripts/test_lua.py suite [--files strings,pm] [--no-dump] [--no-clibs]

# Benchmarks (see benchmarks/lua/README.md)
python3 scripts/test_lua.py bench [--repeat N]
```

CI runs the same check without Python: `cargo test --test lua_corpus` builds
`examples/lua` with the compiler under test and runs every script against its
expected file (`tests/lua_corpus.rs` ports the directives, environment and
stderr rules below). It also builds and runs `examples/lua_embed`.

`--lua` defaults to `build/lua`; `--lua ref` means the reference build.
Arguments to `check`, `diff` and `expect` can be paths, directories, test
names, or name prefixes (`coro_` runs every `coro_*.lua`).

## How a test runs

Each `NAME.lua` runs as `lua NAME.lua`, with `tests/lua` as the working
directory, so chunk names and error positions read `NAME.lua:LINE:`. The
harness removes `LUA_INIT*`, `LUA_PATH*` and `LUA_CPATH*` from the
environment. It sets `LUA_TEST_VAR="hello from env"`, `TZ=UTC` and
`LC_ALL=C`. Stdin is empty unless a directive says otherwise.

The exit status, stdout and stderr are compared:

- stdout must match byte for byte.
- In stderr, a leading interpreter path in `<path>: message` becomes `lua:`,
  and any other occurrence of the interpreter path becomes `lua` (for example
  in usage text).
- By default stderr is compared only up to the first `stack traceback:` line,
  because traceback frames are an implementation detail. Use the `stderr`
  directive or `diff --strict-stderr` for a full comparison.

### Directives

Optional `--! key: value` lines in the leading comment block, before the
first line of code:

| directive | meaning |
|---|---|
| `--! opts: -W -e "x=1"` | interpreter options placed before the script name (shell-style quoting) |
| `--! args: a "b c"` | script arguments placed after the script name |
| `--! cmd: -e "print(1)" - a` | the complete argument list; no script name is added, and `{script}` expands to the file name |
| `--! stdin: fixtures/in.txt` | file fed to stdin, relative to `tests/lua`; `{script}` feeds the test itself (for `lua -`) |
| `--! stderr: full` | compare all of stderr; `message` (default) cuts it at the traceback, `ignore` skips it |

### `.expected` format

```
@@exit 1
@@stdout
...stdout lines...
@@stderr
lua: NAME.lua:3: attempt to index a nil value (local 't')
```

The `@@stderr` section is omitted when stderr is empty. A section header
ending in ` noeol` means the stream did not end with a newline (the stored
text has one added). Tests must not print lines that start with `@@`;
`expect` rejects such output.

## Writing tests

- Write original scripts. The official suite is fetched at test time and run
  with `suite`.
- Output must be deterministic and portable:
  - Never print table, function or thread addresses.
  - Never print the `pairs()` order of hash keys; sort them first.
  - Never print timings, `os.clock()`, current dates, random values (check
    ranges only), `os.tmpname()` paths, `package.path` or
    `collectgarbage("count")`.
  - Print NaN through a helper (`x ~= x and "nan"`), because the sign of
    `-nan` depends on the platform.
  - Print libm results (`sin`, `exp`, `log`, ...) with limited precision
    (`%.10g`).
- Avoid unspecified behaviour: `#t` on tables with holes, `table.sort`
  stability, the result of a sort with an inconsistent comparator, and
  finalizer timing beyond "has run after a full `collectgarbage()`".
- Prefer catching errors with `pcall` and printing the message, so one file
  covers many cases. A file may also end with a deliberate uncaught error; the
  harness records the exit status and the error message.
- Keep each script well under a second on the reference interpreter.
- `expect` runs every script twice and refuses output that differs between
  runs. Review the generated `.expected` file: it must show what you meant to
  test, and not, say, "attempt to call a nil value" caused by a typo.

Fixtures: `modules/` holds modules for the `require_*` and `cli_*` tests
(tests set `package.path` explicitly). `fixtures/` holds chunks for
`load_*`/`cli_*` and stdin inputs.

## Layout

Scripts are grouped by name prefix:

| prefix | area |
|---|---|
| `lex_` | long strings and comments, escapes (`\z`, `\x`, `\u{}`), numerals, lexer errors |
| `arith_`, `bitwise_` | integer and float subtypes, wraparound, `//` and `%`, conversions, coercions, comparisons |
| `string_`, `strlib_`, `pattern_` | float formatting, concatenation, the string library, `string.format`, patterns and `gsub` |
| `pack_`, `utf8_`, `tonumber_` | `string.pack`/`unpack`, the utf8 library, string-to-number conversion |
| `table_`, `tablib_`, `next_` | constructors, length, keys, the table library, `next`/`pairs`/`ipairs` |
| `meta_` | every metamethod, weak tables, finalizers, `__name`, `__metatable`, `__close` |
| `closure_`, `vararg_`, `assign_`, `scope_`, `goto_`, `for_` | upvalues, varargs, assignment order, scoping and `_ENV`, `goto`, numeric and generic `for` |
| `close_`, `const_` | `<close>` and `<const>` variables |
| `coro_` | coroutines, including yields across `pcall`, metamethods and iterators |
| `error_`, `errmsg_` | `error`/`pcall`/`xpcall`, uncaught error objects, runtime error messages with variable names |
| `load_`, `require_` | `load`, chunk names, modes, environments, `string.dump`, `require`, `package` |
| `io_`, `os_`, `math_`, `debug_`, `gc_` | standard libraries |
| `cli_` | the stand-alone interpreter: `-e -l -W -E - --`, `arg`, exit statuses, usage errors, the REPL banner (`cli_banner` only) |
| `prog_` | larger programs: JSON, OOP, Brainfuck, sudoku, LRU, text processing, Dijkstra, bigint, Life, an expression parser, a scheduler, a serializer |

## Notes on the reference build

- `fetch` builds the reference like `make macosx` but without readline on
  macOS, and with `make linux` (also without readline) on Linux, so
  interactive-mode output does not depend on libedit or GNU readline. The
  default build defines `LUA_COMPAT_5_3`: `math.pow` and friends exist, and
  `__le` falls back to `not __lt`. The corpus records that behaviour.
- The expected files were generated on macOS. They avoid known libc
  differences such as `%a` rounding and subnormal formatting.
- `cli_banner` is the only test that prints the version banner
  (`Lua 5.4.9  Copyright ...`).

## The official test suite (`suite`)

`suite` runs each file of `lua-5.4.9-tests` in its own process, from a
scratch copy of the suite directory. It uses a generated driver that mirrors
`all.lua`:

- It sets `_soft`, `_port` and `_nomsg`, sets `debug = nil` and
  `T = rawget(_G, "T")`, defines `Message`, and calls `os.setlocale"C"`.
- It loads each file the way `all.lua` does: through
  `string.dump`/`load` (stripped for `code.lua`, `goto.lua`, `sort.lua` and
  `verybig.lua`), plain source for `gc.lua`, `strings.lua` and `literals.lua`,
  and inside a coroutine for `big.lua`.
- It checks the return values `all.lua` asserts (`calls.lua`, `attrib.lua`,
  `locals.lua`, `events.lua`, `verybig.lua`).

Default settings: `_soft=true` and `_port=true`, except `_port=false` for
`main.lua`, which tests the stand-alone interpreter. `--no-soft` and
`--no-port` turn the long and the non-portable tests on.

Files that need the internal test library `T` (an `ltests` build) skip those
parts. `api.lua` and `code.lua` skip entirely and are reported as SKIP.

Harness patches applied to the scratch copy (listed in the output):

- `main.lua` accepts interactive output without GNU readline's echo of piped
  input.
- With `--no-clibs`, `main.lua` skips loading the C module `libs/lib2-v2.so`.

`--no-dump` loads files from source, for implementations without binary
chunks.
