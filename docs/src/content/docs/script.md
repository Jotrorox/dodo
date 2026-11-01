---
title: "Embedded scripting with Lua"
description: "Run Lua 5.4 programs inside a Dodo application, call between Dodo and Lua, and use the standalone lua interpreter."
section: "Standard library"
order: 160
---

`std/script/lua` is a complete Lua 5.4 implementation written in Dodo. It runs
Lua source and binary chunks (compatible with the reference `luac` 5.4), with
the full language: integer and float numbers, metatables, closures, varargs,
to-be-closed variables, coroutines, weak tables, finalizers and an incremental
garbage collector. The embedding API follows the shape of the Lua C API, so
code and documentation written for it translate directly.

| Goal | Starting point |
| --- | --- |
| Create a Lua state with the standard libraries | `lua.Lua.new()` and `open_libs()` |
| Run a chunk of source or a binary chunk | `cx.load_bytes(...)` then `cx.pcall(...)` |
| Expose a Dodo function to Lua | a `lua.NativeFn` and `cx.push_native` / `cx.set_global` |
| Call a Lua function from Dodo | `cx.get_global(...)`, push arguments, `cx.pcall(...)` |
| Check and convert arguments in a native function | `cx.check_integer`, `cx.check_lstr`, `cx.opt_number`, ... |
| Store Dodo data in Lua | `cx.new_userdata(size, user_values)` with a metatable |
| Run the standalone interpreter | `dodo build examples/lua -O 2 -o build/lua` |

## A state and its handle

`Lua` owns a universe: the global state and its main thread. Dropping it
closes the state, runs pending finalizers and frees every object. `Cx` is a
thin handle on one Lua thread; `lua.cx()` returns one for the main thread and
every native function receives one.

```dodo
package main

import "std/script/lua"

fn main() -> i32 {
    state := lua.Lua.new()!
    state.open_libs()!
    cx := state.cx()
    let status = cx.load_bytes(b"return 6 * 7", b"=example", b"t")
    if status != lua.OK || cx.pcall(0, 1, 0) != lua.OK {
        return 1
    }
    match cx.to_integer(-1) {
        some(v) => { return (v - 42) as i32 },
        none => { return 1 },
    }
}
```

Values live on a stack, addressed like in the C API: `1..n` from the bottom
of the current frame, `-1..-n` from the top, and the pseudo-indices
`lua.REGISTRY_INDEX` and `lua.upvalue_index(i)`.

## Native functions

A native function has the type `lua.NativeFn = fn(&mut lua.Cx) -> i32!lua.Throw`.
It reads its arguments from the stack, pushes its results and returns how
many it pushed. Raising a Lua error is an ordinary `err` return:

```dodo
fn clamp(cx: &mut lua.Cx) -> i32!lua.Throw {
    let v = cx.check_number(1)?
    let lo = cx.opt_number(2, 0.0)?
    let hi = cx.opt_number(3, 1.0)?
    if lo > hi {
        return err(cx.error_msg("empty range"))
    }
    cx.push_number(if v < lo { lo } else if v > hi { hi } else { v })
    return ok(1)
}
```

Register it with `cx.push_native(clamp)?` followed by `cx.set_global("clamp")?`,
or build a library table with `cx.new_lib(&[("clamp", clamp)])?`. Argument
errors use the reference wording, for example
`bad argument #1 to 'clamp' (number expected, got nil)`.

Every operation that can raise a Lua error (allocation, metamethods, calls)
returns `T!lua.Throw`; propagate it with `?`. From embedding code, run Lua
through `pcall`, which returns a status (`lua.OK`, `lua.ERRRUN`, ...) and
leaves the error object on the stack.

## Coroutines and continuations

Lua coroutines work across native frames. A native function that calls Lua
and may be interrupted by a yield uses `call_k` or `pcall_k` with a
continuation; a native function yields with `return err(cx.yield_(n))` or
`cx.yield_k(n, ctx, k)`. Embedding code resumes a thread with `cx.resume`.

## Memory

All memory is accounted. `state.set_memory_limit(bytes)` caps the state:
an allocation beyond the limit first triggers an emergency collection, then
raises `not enough memory`. `state.close()` closes the state immediately and
returns the number of bytes that were not released (zero unless something
leaked).

## The standalone interpreter

`examples/lua` is a drop-in equivalent of the reference `lua` program:

```sh
dodo build examples/lua -O 2 -o build/lua
build/lua script.lua arg1 arg2
build/lua -e 'print(_VERSION)'
build/lua -i
```

It supports `-e`, `-l`, `-i`, `-v`, `-E`, `-W`, `-` and `--`, the global
`arg` table, `LUA_INIT_5_4`/`LUA_INIT`, binary chunks and scripts that start
with a `#!` line. Errors are reported on standard error as `lua: message`
followed by a traceback, with exit status 1.

## Standard libraries and platforms

`open_libs()` opens the base, package, coroutine, string, utf8, table, math,
io, os and debug libraries. The io and os libraries wrap the C library, so
they follow its behaviour on each system, as the reference interpreter
does:

- `io.stdin`, `io.stdout` and `io.stderr` are the C library's standard
  streams; `print` writes to the same stdout, so output stays ordered, and
  stderr is unbuffered.
- `io.popen` and `os.execute` run commands through the system shell
  (`/bin/sh` on Linux and macOS, `cmd.exe` on Windows). On Linux and macOS a
  process killed by a signal reports `nil, "signal", signo`; on Windows the
  status is always the exit code. Pending output is flushed before a
  command starts.
- `os.date` accepts the strftime conversions of ISO C99 and POSIX (with the
  `E` and `O` modifiers) on Linux and macOS, and the conversions of the
  Windows C runtime (with the `#` flag) on Windows; others raise
  `invalid conversion specifier`.
- `os.tmpname` creates the file (`/tmp/lua_XXXXXX`) on Linux and macOS, so
  the name cannot be taken by another process; on Windows it only returns a
  name. File positions are 64-bit everywhere.
- Error results use the C library's messages and errno values, for example
  `nil, "x.txt: No such file or directory", 2`.

The per-system C differences live in the `std/script/native` adapters, which
resolve to `std/script/posix` or `std/script/windows` for the compilation
target.
