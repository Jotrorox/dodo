---
title: "Embedded scripting with Lua"
description: "Run Lua 5.4 programs inside a Dodo application, exchange values and calls between Dodo and Lua, and use the standalone lua interpreter."
section: "Standard library"
order: 160.9
---

`std/script/lua` is a complete Lua 5.4 implementation written in Dodo. A Dodo
program can embed it to run user scripts, configuration or plugins, and
`examples/lua` builds it into a standalone `lua` interpreter. It implements the
whole language: integers and floats, metatables, closures, varargs, `goto`,
to-be-closed variables, coroutines, weak tables, finalizers and an
incremental and generational garbage collector, with the standard libraries
`base`, `package`, `coroutine`, `string`, `utf8`, `table`, `math`, `io`, `os`
and `debug`. It loads binary chunks produced by the reference `luac` 5.4.

The embedding API follows the shape of the Lua C API, so documentation and
experience from C translate directly: `lua_getfield` is `cx.get_field`,
`luaL_checkinteger` is `cx.check_integer`, `lua_pcall` is `cx.pcall`. The
[API page](api/std/script/lua.md) lists every declaration.

| Goal | Starting point |
| --- | --- |
| Create a state with the standard libraries | `lua.Lua.new()` and `state.open_libs()` |
| Open only some libraries | `cx.require_lib("string", lua.open_string, true)` |
| Run a string or a file | `cx.do_string(...)`, `cx.load_buffer(...)` or `cx.load_file(...)`, then `cx.pcall(...)` |
| Read and write values | the stack: `cx.push_*`, `cx.to_*`, `cx.get_field`, `cx.set_global`, ... |
| Call a Lua function from Dodo | `cx.get_global(...)`, push arguments, `cx.pcall(nargs, nresults, msgh)` |
| Expose a Dodo function to Lua | a `lua.NativeFn` and `cx.push_native` / `cx.new_lib` |
| Check arguments in a native function | `cx.check_integer`, `cx.check_lstr`, `cx.opt_number`, ... |
| Store Dodo data in Lua | `cx.new_userdata(size, 0)` with a metatable from `cx.new_metatable` |
| Make a Dodo module available to `require` | `state.preload_module("name", open_fn)` |
| Drive a coroutine | `cx.new_thread()` and `cx.resume(...)` |
| Bound memory or running time | `state.set_memory_limit(...)`, a count hook with `cx.set_hook(...)` |
| Run the standalone interpreter | `dodo build examples/lua -O 2 -o build/lua` |

The package is hosted: it runs on x86-64 and AArch64 Linux (GNU), macOS, and
x86-64 Windows, and links the target's C library for `io`, `os` and number
formatting. See [platforms](#platforms) below.

## Quickstart

Save this as `lua_start.dodo` and run it with `dodo run lua_start.dodo`:

```dodo test
package lua_start

import "std/script/lua"

fn main() -> i32 {
    match run() {
        ok(answer) => {
            assert_eq(answer, 42i64)
            return 0
        },
        err(_) => {
            return 1
        },
    }
}

fn run() -> i64!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    cx := state.cx()
    if cx.do_string(b"answer = #('forty-two'):rep(4, '..')") != lua.OK {
        return err(lua.Throw.Runtime)
    }
    cx.get_global("answer")?
    match cx.to_integer(-1) {
        some(v) => {
            return ok(v)
        },
        none => {
            return err(lua.Throw.Runtime)
        },
    }
}
```

`lua.Lua.new()` creates a state with no libraries; `open_libs` opens all of
them into the global table. `do_string` compiles and runs a chunk in protected
mode and returns a status (`lua.OK` on success). `get_global` pushes the value
of a global onto the stack and `to_integer` reads it back.

`examples/lua_embed` is a longer program: it registers a Dodo function and a
module, runs a script, reads a result table, calls a Lua function and reports a
Lua error. Run it with `dodo run examples/lua_embed` from a source checkout.

## States and handles

`lua.Lua` owns a universe: the global state, its main thread and every object
in it. Dropping the value closes the state, runs pending finalizers (`__gc`
metamethods) and frees all memory. `state.close()` does the same immediately
and returns the number of bytes that were not released, which is zero unless
something leaked.

`lua.Cx` is a small handle on one Lua thread. `state.cx()` returns a handle on
the main thread, every native function receives one for the calling thread,
and `cx.new_thread()` returns one for a new coroutine. A handle does not own
anything and is valid only while its thread is alive. Copies are cheap; take
one wherever a `&mut lua.Cx` is needed.

Separate `Lua` values share no Lua objects, but they do share process-wide C
library state such as the locale and the standard streams. Use a state from
one thread at a time.

### Opening libraries

`open_libs` opens every library. To run untrusted scripts, open only the
libraries they need with `require_lib`, which calls a library's opener, stores
the result in `package.loaded` and, when the last argument is true, sets the
global of the same name. It leaves the library table on the stack:

```dodo
cx.require_lib("_G", lua.open_base, true)?
cx.pop(1)
cx.require_lib("string", lua.open_string, true)?
cx.pop(1)
cx.require_lib("table", lua.open_table, true)?
cx.pop(1)
cx.require_lib("math", lua.open_math, true)?
cx.pop(1)
```

The openers are `open_base`, `open_package`, `open_coroutine`, `open_string`,
`open_utf8`, `open_table`, `open_math`, `open_io`, `open_os` and `open_debug`.
The base library alone still provides `load`, `dofile` and `loadfile`; remove
globals you do not want after opening it, for example with
`cx.push_nil()` and `cx.set_global("dofile")?`. Without `io`, `os` and
`package`, a script cannot touch files, processes or the environment.

## Running code

`cx.load_buffer(chunk, chunkname, mode)` compiles a chunk and pushes it as a
function, or pushes an error message and returns `lua.ERRSYNTAX` (or
`lua.ERRMEM`). The chunk name appears in error messages and tracebacks: `=name`
is used verbatim, `@file.lua` is a file name, anything else is shown as
`[string "..."]`. `mode` is `"t"` (text only), `"b"` (binary chunks only) or
`"bt"`. Accept binary chunks only from trusted sources: like the reference
implementation, the loader does not verify bytecode.

`cx.load_bytes` takes the name and mode as byte strings. `cx.load_file(path,
mode)` reads a file (`none` reads standard input), skips a first line starting
with `#`, detects binary chunks, and returns `lua.ERRFILE` with a message such
as `cannot open config.lua: No such file or directory` when the file cannot
be read.

A loaded chunk is an ordinary function with no parameters. Run it with
`cx.pcall(nargs, nresults, msgh)`:

```dodo
if cx.load_buffer(source, "=settings", "t") != lua.OK || cx.pcall(0, lua.MULTRET_ALL, 0) != lua.OK {
    // the error message is on top of the stack
}
```

`cx.do_string(s)` combines both steps, using the chunk as its own name and
`MULTRET_ALL` results. `cx.dump(strip)` turns the Lua function on top of the
stack into a binary chunk (pushed as a string), which `load_buffer` with mode
`"b"` and the reference `lua` both accept.

## The stack

Values pass between Dodo and Lua through a stack, as in the C API. Index `1`
is the bottom of the current frame (the first argument of a native function),
`-1` the top. `lua.REGISTRY_INDEX` addresses the registry, a table only native
code can reach, and `lua.upvalue_index(i)` the upvalues of the running native
closure. `cx.get_top()`, `cx.set_top(n)`, `cx.pop(n)`, `cx.push_value(i)`,
`cx.insert(i)`, `cx.remove(i)`, `cx.replace(i)`, `cx.rotate(i, n)` and
`cx.copy(from, to)` rearrange it. As in C, the stack does not grow by
itself: embedding code and every native function can rely on 20 free slots,
and must call `cx.check_stack(n)` (false when the stack cannot grow) before
pushing more.

| Lua type | Push | Read |
| --- | --- | --- |
| nil | `push_nil()` | `is_nil(i)`, `is_none_or_nil(i)` |
| boolean | `push_boolean(b)` | `to_boolean(i)` (false only for nil and false) |
| integer | `push_integer(n)` | `to_integer(i) -> Option<i64>` (converts floats with an exact integer value and numeric strings) |
| float | `push_number(x)` | `to_number(i) -> Option<f64>` |
| string | `push_str(s)?`, `push_bytes(b)?` | `to_lstr(i)? -> Option<LuaStr>` (converts numbers in place, like `lua_tolstring`) |
| table | `new_table()?`, `create_table(narr, nrec)?` | `get_field`, `get_i`, `get_table`, `next`, `raw_len` |
| function | `push_native(f)?`, `push_closure(f, n)?` | `is_function(i)`, call it |
| userdata | `new_userdata(size, nuv)?` | `to_userdata(i)`, `check_udata(i, name)?` |
| thread | `new_thread()?` | `to_thread(i) -> Option<Cx>` |

`cx.type_of(i)` returns `lua.TNIL`, `lua.TNUMBER`, `lua.TSTRING`, ... and
`lua.TNONE` for an index above the top. Lua strings are byte strings and may
contain zeros; a `LuaStr` gives access to them with `bytes()` and stays valid
while the string stays on the stack, independently of later calls on `cx`.
`cx.to_lstring_meta(i)?` converts any value like `tostring`, honouring
`__tostring` and `__name`.

Operations that may run Lua code or allocate return `T!lua.Throw`:
`get_field` and `get_table` can call `__index`, `set_field` can call
`__newindex`, `compare`, `arith`, `concat` and `len` can call metamethods. The
`raw_*` variants (`raw_get`, `raw_get_i`, `raw_set`, `raw_set_i`, `raw_len`,
`raw_equal`) bypass metamethods. Read a table built by a script:

```dodo
assert_eq(cx.do_string(b"return {name = 'probe', tags = {'a', 'b', 'c'}}"), lua.OK)
assert_eq(cx.get_field(-1, "name")?, lua.TSTRING)   // pushes the field, returns its type
cx.pop(1)
cx.get_field(-1, "tags")?
assert_eq(cx.raw_len(-1), 3u64)
cx.push_nil()                                        // first key
for cx.next(-2)? {                                   // pushes key and value
    // key at -2, value at -1
    cx.pop(1)                                        // keep the key for next
}
```

Build one for a script to read:

```dodo
cx.create_table(0, 2)?
cx.push_str("dodo")?
cx.set_field(-2, "lang")?
cx.push_integer(5)
cx.set_field(-2, "version")?
cx.set_global("host")?        // Lua: host.lang == "dodo"
```

### Keeping values between calls

Values on the stack of a native function are released when it returns. To
keep a Lua value, such as a callback a script registered, store it in the
registry: `cx.reference(lua.REGISTRY_INDEX)?` pops the top value and returns an
integer handle, `cx.raw_get_i(lua.REGISTRY_INDEX, r as i64)` pushes it again,
and `cx.unreference(lua.REGISTRY_INDEX, r)?` releases it. Never keep raw
pointers to Lua objects (other than full userdata blocks you also keep
reachable) in Dodo memory: the collector only sees values reachable from Lua.

## Calling Lua from Dodo

Push the function, then its arguments, and call it. `nresults` is the number
of results you want (missing ones are nil, extra ones dropped), or
`lua.MULTRET_ALL`:

```dodo
cx.get_global("greet")?
cx.push_str("ada")?
cx.push_integer(2)
if cx.pcall(2, 2, 0) == lua.OK {
    // the first result at -2, the second at -1
}
```

`pcall` catches every error and returns its status. `cx.call(nargs,
nresults)?` is the unprotected form for use inside native functions: an error
propagates as an `err(lua.Throw)` to whoever called the native function.

## Calling Dodo from Lua

A native function has the type `lua.NativeFn`, `fn(&mut lua.Cx) ->
i32!lua.Throw`. It finds its arguments at stack indices `1..cx.get_top()`,
pushes its results and returns how many it pushed:

```dodo
// divmod(a, b) -> a // b, a % b
fn divmod(cx: &mut lua.Cx) -> i32!lua.Throw {
    let a = cx.check_integer(1)?
    let b = cx.check_integer(2)?
    cx.arg_check(b != 0, 2, "zero divisor")?
    cx.push_integer(a / b)
    cx.push_integer(a % b)
    return ok(2)
}
```

Register it as a global with `cx.push_native(divmod)?` and
`cx.set_global("divmod")?`, or group functions in a table with
`cx.new_lib(&funcs)?`, where `funcs` is an array of `("name", function)`
pairs. `cx.set_funcs(&funcs, n)?` adds functions to the table below `n`
upvalues instead.

The checking helpers raise errors worded like the reference implementation:
`check_integer`, `check_number`, `check_lstr`, `check_any`, `check_type`,
`check_option` and `check_udata`, the optional forms `opt_integer`,
`opt_number` and `opt_lstr`, and `arg_check`, `arg_error` and `type_error`.
`divmod('x')` fails with `bad argument #1 to 'divmod' (number expected, got
string)`, `divmod(1, 0)` with `bad argument #2 to 'divmod' (zero divisor)`.

A native closure carries upvalues, Lua values stored with the function.
`cx.push_closure(f, n)?` pops `n` values into a new closure, and the function
addresses them with `lua.upvalue_index(i)`:

```dodo
// Each call returns the next integer.
fn next_id(cx: &mut lua.Cx) -> i32!lua.Throw {
    let n = int_at(cx, lua.upvalue_index(1)) + 1
    cx.push_integer(n)
    cx.copy(-1, lua.upvalue_index(1))
    return ok(1)
}

cx.push_integer(0)
cx.push_closure(next_id, 1)?
cx.set_global("next_id")?
```

A `NativeFn` is a plain function pointer, not a closure over Dodo values:
keep per-state data in upvalues, the registry or userdata rather than in Dodo
globals.

## Errors

There is no unwinding: every operation that can raise a Lua error returns
`T!lua.Throw`, and the error travels as an ordinary `err` through `?` until a
protected call catches it. The `Throw` value tells the kind of error
(`Runtime`, `Syntax`, `Memory`, `Handler`) and the error object itself is on
the Lua stack. A native function raises an error by returning one:

- `return err(cx.error_msg("empty range"))` raises a string with the position
  of the calling Lua code prepended, like `luaL_error`
  (`config:3: empty range`).
- `return err(cx.error())` raises the value on top of the stack, which may be
  any Lua value, such as a table with an error code.
- `cx.arg_error(n, msg)` and `cx.type_error(n, tname)` describe a bad argument.

Protected calls (`pcall`, `do_string`, `resume`) return a status: `lua.OK`,
`lua.ERRRUN` (runtime error), `lua.ERRSYNTAX`, `lua.ERRMEM` (out of memory) or
`lua.ERRERR` (error in the message handler), and leave the error object on the
stack. The third argument of `pcall` is the stack index of a message handler,
a function called with the error object before the stack unwinds; use it to
add a traceback:

```dodo
fn add_traceback(cx: &mut lua.Cx) -> i32!lua.Throw {
    match cx.to_lstr(1)? {
        some(msg) => {
            th := cx.thread_handle()
            cx.traceback(&mut th, some(msg.bytes()), 1)?
        },
        none => {
            cx.push_value(1)    // not a string: keep the error object
        },
    }
    return ok(1)
}

cx.push_native(add_traceback)?            // handler at index 1
cx.load_buffer(b"local t = nil\nreturn t.x", "=job", "t")
let status = cx.pcall(0, 0, 1)            // lua.ERRRUN
// error object: "job:2: attempt to index a nil value (local 't')\nstack traceback:\n..."
```

Run script code only through protected calls. An unprotected operation that
fails when called directly from embedding code, outside any `pcall`, returns
its `err`, but like a panic in the C API it leaves the state in an unspecified
condition: close it. To do work that may raise, such as calling metamethods,
put it in a native function and call that with `pcall`.

## Userdata

Full userdata is a block of memory owned by Lua, which Dodo code can use to
store a struct. Give it a metatable to add methods, operators, `__tostring`,
`__close` or a `__gc` finalizer. `cx.new_metatable(name)?` creates a metatable
registered under a type name (and records the name in its `__name` field),
`cx.set_metatable_by_name(name)?` attaches it, and `cx.check_udata(i, name)?`
returns the block, raising `bank.Account expected, got table` for a value of
another type:

```dodo
struct Account {
    balance: i64
    deposits: i64
}

const ACCOUNT: &str = "bank.Account"

fn account_new(cx: &mut lua.Cx) -> i32!lua.Throw {
    let initial = cx.opt_integer(1, 0)?
    let block = cx.new_userdata(mem.size_of::<Account>(), 0)?
    unsafe {
        *(block as *mut Account) = Account { balance: initial, deposits: 0 }
    }
    cx.set_metatable_by_name(ACCOUNT)?
    return ok(1)
}

fn open_bank(cx: &mut lua.Cx) -> i32!lua.Throw {
    // The metatable: methods are found through __index.
    cx.new_metatable(ACCOUNT)?
    let methods = [("deposit", account_deposit), ("balance", account_balance), ("__tostring", account_tostring)]
    cx.set_funcs(&methods, 0)?
    cx.push_value(-1)
    cx.set_field(-2, "__index")?
    cx.pop(1)
    let funcs = [("new", account_new)]
    cx.new_lib(&funcs)?
    return ok(1)
}
```

The block is 16-byte aligned and lives until the collector frees
the userdata, after running its `__gc` metamethod if it has one. Lua does not
know what the block contains: store plain data (numbers, flags, raw handles),
not Dodo values that own memory or need `drop`, and release external
resources from `__gc` or `__close`. The second argument of `new_userdata` is
the number of user values, Lua values attached to the userdata
(`cx.get_iuservalue`, `cx.set_iuservalue`); use them to keep other Lua objects
alive with it. `cx.push_light_userdata(p)` pushes a bare pointer that Lua
neither owns nor tracks.

## Modules and `require`

`state.preload_module(name, open)` (or `cx.preload_module`) registers a Dodo
module for `require`, before or after the libraries are opened. `require` calls
the opener like a module loader, with the module name and `":preload:"`, and
caches its first result in `package.loaded`:

```dodo
state.preload_module("bank", open_bank)?
// Lua: local bank = require "bank"; local acct = bank.new(10):deposit(5)
```

`require` otherwise follows the reference searchers: `package.preload`, Lua
files along `package.path`, then the C searchers along `package.cpath`. There
is no C API, so C libraries cannot be loaded: the C searchers still list the
files they try, a file that exists fails with `dynamic libraries not enabled;
check your Lua installation`, and `package.loadlib` returns `nil`, that
message and `"absent"`, as in a reference build without dynamic-library
support. `package.path` and `package.cpath` start from `LUA_PATH_5_4` or
`LUA_PATH` and `LUA_CPATH_5_4` or `LUA_CPATH` (`;;` stands for the default
path) unless the registry field `LUA_NOENV` is true, which the interpreter's
`-E` option sets.

## Coroutines

Create a coroutine with `cx.new_thread()?`, which pushes the thread on the
current stack (keeping it alive) and returns its handle. Push the body and its
arguments on the new thread, then resume it repeatedly:

```dodo
co := cx.new_thread()?
co.get_global("producer")?
co.push_integer(3)
nres := 0i32
nargs := 1i32
for {
    let status = cx.resume(&mut co, nargs, &mut nres)
    if status == lua.YIELD {
        // nres yielded values on top of co's stack
        co.pop(nres)
        nargs = 0          // values pushed on co before the next resume are passed to yield
    } else {
        break              // lua.OK: returned; otherwise an error status
    }
}
```

Coroutines work across native frames. A native function yields with `return
err(cx.yield_(n))`, which yields its top `n` values; `cx.yield_k(n, ctx, k)`
yields and calls the continuation `k` with `ctx` when the coroutine resumes. A
native function that calls Lua code that may yield uses `cx.call_k` or
`cx.pcall_k` with a continuation, so the call can resume after a yield; with
plain `cx.call`, a yield fails with `attempt to yield across a C-call
boundary`. A `lua.ContinuationFn` is `fn(&mut lua.Cx, status: i32, ctx:
isize) -> i32!lua.Throw`.

## Memory, warnings and limits

All memory a state uses is counted: `state.memory_in_use()` reports it and
`collectgarbage("count")` shows the same figure to scripts.
`state.set_memory_limit(bytes)` caps it (0 means unlimited). An allocation
beyond the limit first triggers an emergency collection and then raises a
memory error, so the protected call returns `lua.ERRMEM` with the message
`not enough memory`. `cx.gc_collect()`, `cx.gc_stop()`, `cx.gc_restart()`,
`cx.gc_step(kb)`, `cx.gc_incremental(...)` and `cx.gc_generational(...)`
control the collector like `collectgarbage`.

Scripts emit warnings with `warn`. They are off by default, and `warn("@on")`
turns them on; the default handler then prints `Lua warning: ...` on standard
error. `state.set_warn_fn(f)` installs a `lua.WarnFn`, `fn(&mut lua.Cx, msg:
&[u8], tocont: bool)`, which receives every piece, including control messages
such as `@on`; `tocont` is true when more pieces of the same warning follow.
`cx.warning(msg, tocont)` emits one from Dodo.

To bound the running time of a script, install a count hook that raises an
error. The hook runs every `count` instructions; returning an error from it
aborts the running code, and the protected call reports it:

```dodo
fn out_of_budget(cx: &mut lua.Cx, ar: &mut lua.Debug) -> void!lua.Throw {
    return err(cx.error_msg("instruction budget exhausted"))
}

cx.set_hook(out_of_budget, lua.MASKCOUNT, 100000)
cx.do_string(b"while true do end")    // lua.ERRRUN: instruction budget exhausted
cx.clear_hook()
```

`cx.watch_interrupt(true)` makes the first Ctrl-C (SIGINT) raise
`interrupted!` in the Lua code running on that thread at its next jump, loop
or call, as the standalone interpreter does; the handler then restores the
default action, so a second Ctrl-C ends a program stuck in native code. Turn it
off with `cx.watch_interrupt(false)` once the code has finished.

## A complete host program

This program exercises everything described above: selected libraries,
native functions and closures, calls in both directions, tables, references, errors with a
traceback handler, userdata with methods in a `require`-able module,
coroutines, warnings, an instruction budget and a memory limit. Each test
creates its own state.

```dodo test
package lua_host

import "core/mem"
import "std/script/lua"

// --- helpers -------------------------------------------------------------

// Fail the test on an error raised outside a protected call.
fn must(r: void!lua.Throw) {
    match r {
        ok() => {},
        err(_) => {
            assert(false)
        },
    }
}

fn str_eq(a: &[u8], b: &str) -> bool {
    let c = mem.str_bytes(b)
    if a.len != c.len {
        return false
    }
    for i in 0usize..a.len {
        if a[i] != c[i] {
            return false
        }
    }
    return true
}

// The string at idx, compared with want (false for non-strings).
fn top_is(cx: &mut lua.Cx, idx: i32, want: &str) -> bool {
    match cx.to_lstr(idx) {
        ok(o) => {
            match o {
                some(s) => {
                    if !str_eq(s.bytes(), want) {
                        lua.write_stderr(b"got: ")
                        lua.write_stderr(s.bytes())
                        lua.write_stderr(b"\n")
                        return false
                    }
                    return true
                },
                none => {
                    return false
                },
            }
        },
        err(_) => {
            return false
        },
    }
}

fn int_at(cx: &lua.Cx, idx: i32) -> i64 {
    match cx.to_integer(idx) {
        some(v) => {
            return v
        },
        none => {
            return -1
        },
    }
}

// --- selected libraries --------------------------------------------------

@test
fn selected_libraries() {
    must(selected_libraries_run())
}

fn selected_libraries_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    cx := state.cx()
    cx.require_lib("_G", lua.open_base, true)?
    cx.pop(1)
    cx.require_lib("string", lua.open_string, true)?
    cx.pop(1)
    cx.require_lib("table", lua.open_table, true)?
    cx.pop(1)
    cx.require_lib("math", lua.open_math, true)?
    cx.pop(1)
    cx.push_nil()
    cx.set_global("dofile")?
    assert_eq(cx.do_string(b"assert(io == nil and os == nil and require == nil and dofile == nil)"), lua.OK)
    assert_eq(cx.do_string(b"assert(('%d'):format(math.max(2, 3)) == '3' and table.concat({1, 2}, '-') == '1-2')"), lua.OK)
    return ok()
}

// --- native functions ----------------------------------------------------

// divmod(a, b) -> a // b, a % b
fn divmod(cx: &mut lua.Cx) -> i32!lua.Throw {
    let a = cx.check_integer(1)?
    let b = cx.check_integer(2)?
    cx.arg_check(b != 0, 2, "zero divisor")?
    cx.push_integer(a / b)
    cx.push_integer(a % b)
    return ok(2)
}

// A closure with one upvalue: each call returns the next integer.
fn next_id(cx: &mut lua.Cx) -> i32!lua.Throw {
    let n = int_at(cx, lua.upvalue_index(1)) + 1
    cx.push_integer(n)
    cx.copy(-1, lua.upvalue_index(1))
    return ok(1)
}

@test
fn native_functions() {
    must(native_functions_run())
}

fn native_functions_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    cx := state.cx()
    cx.push_native(divmod)?
    cx.set_global("divmod")?
    cx.push_integer(0)
    cx.push_closure(next_id, 1)?
    cx.set_global("next_id")?
    let funcs = [("divmod", divmod)]
    cx.new_lib(&funcs)?
    cx.set_global("mathx")?

    assert_eq(cx.do_string(b"q, r = divmod(17, 5); a = next_id(); b = next_id(); c = mathx.divmod(9, 2)"), lua.OK)
    cx.get_global("q")?
    cx.get_global("r")?
    cx.get_global("b")?
    assert_eq(int_at(&cx, -3), 3i64)
    assert_eq(int_at(&cx, -2), 2i64)
    assert_eq(int_at(&cx, -1), 2i64)
    cx.set_top(0)

    assert_eq(cx.do_string(b"return divmod(1, 0)"), lua.ERRRUN)
    assert(top_is(&mut cx, -1, "[string \"return divmod(1, 0)\"]:1: bad argument #2 to 'divmod' (zero divisor)"))
    cx.pop(1)
    assert_eq(cx.do_string(b"return divmod('x')"), lua.ERRRUN)
    assert(top_is(&mut cx, -1, "[string \"return divmod('x')\"]:1: bad argument #1 to 'divmod' (number expected, got string)"))
    cx.pop(1)
    return ok()
}

// --- calling Lua ---------------------------------------------------------

@test
fn calling_lua() {
    must(calling_lua_run())
}

fn calling_lua_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    cx := state.cx()
    assert_eq(cx.do_string(b"function greet(name, n) return ('hi ' .. name):rep(n, ', '), n * 2 end"), lua.OK)
    cx.get_global("greet")?
    cx.push_str("ada")?
    cx.push_integer(2)
    assert_eq(cx.pcall(2, 2, 0), lua.OK)
    assert(top_is(&mut cx, -2, "hi ada, hi ada"))
    assert_eq(int_at(&cx, -1), 4i64)
    cx.pop(2)

    // A table result: read fields and walk it with next.
    assert_eq(cx.do_string(b"return {name = 'probe', tags = {'a', 'b', 'c'}}"), lua.OK)
    assert_eq(cx.get_field(-1, "name")?, lua.TSTRING)
    assert(top_is(&mut cx, -1, "probe"))
    cx.pop(1)
    cx.get_field(-1, "tags")?
    assert_eq(cx.raw_len(-1), 3u64)
    cx.get_i(-1, 2)?
    assert(top_is(&mut cx, -1, "b"))
    cx.pop(1)
    count := 0
    cx.push_nil()
    for cx.next(-2)? {
        count += 1
        cx.pop(1)
    }
    assert_eq(count, 3)
    cx.set_top(0)

    // A table built from Dodo.
    cx.create_table(0, 2)?
    cx.push_str("dodo")?
    cx.set_field(-2, "lang")?
    cx.push_integer(5)
    cx.set_field(-2, "version")?
    cx.set_global("host")?
    assert_eq(cx.do_string(b"assert(host.lang == 'dodo' and host.version == 5)"), lua.OK)

    // Keep a Lua function in the registry and call it later.
    assert_eq(cx.do_string(b"return function(x) return x + 1 end"), lua.OK)
    let callback = cx.reference(lua.REGISTRY_INDEX)?
    cx.raw_get_i(lua.REGISTRY_INDEX, callback as i64)
    cx.push_integer(41)
    assert_eq(cx.pcall(1, 1, 0), lua.OK)
    assert_eq(int_at(&cx, -1), 42i64)
    cx.pop(1)
    cx.unreference(lua.REGISTRY_INDEX, callback)?
    return ok()
}

// --- errors --------------------------------------------------------------

// Message handler: append a traceback to the error message.
fn add_traceback(cx: &mut lua.Cx) -> i32!lua.Throw {
    match cx.to_lstr(1)? {
        some(msg) => {
            th := cx.thread_handle()
            cx.traceback(&mut th, some(msg.bytes()), 1)?
        },
        none => {
            cx.push_value(1)
        },
    }
    return ok(1)
}

@test
fn errors() {
    must(errors_run())
}

fn errors_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    cx := state.cx()
    // Syntax errors come from loading.
    assert_eq(cx.load_buffer(b"x = = 1", "=settings", "t"), lua.ERRSYNTAX)
    assert(top_is(&mut cx, -1, "settings:1: unexpected symbol near '='"))
    cx.pop(1)
    // Runtime errors from pcall, here with a message handler at index 1.
    cx.push_native(add_traceback)?
    assert_eq(cx.load_buffer(b"local t = nil\nreturn t.x", "=job", "t"), lua.OK)
    assert_eq(cx.pcall(0, 0, 1), lua.ERRRUN)
    match cx.to_lstr(-1)? {
        some(msg) => {
            let b = msg.bytes()
            let first = "job:2: attempt to index a nil value (local 't')\nstack traceback:"
            assert(b.len > mem.str_bytes(first).len)
            assert(str_eq(b[0..mem.str_bytes(first).len], first))
        },
        none => {
            assert(false)
        },
    }
    cx.set_top(0)
    // Any value can be an error object.
    assert_eq(cx.do_string(b"error({code = 7})"), lua.ERRRUN)
    assert(cx.is_table(-1))
    cx.get_field(-1, "code")?
    assert_eq(int_at(&cx, -1), 7i64)
    cx.set_top(0)
    return ok()
}

// --- userdata ------------------------------------------------------------

struct Account {
    balance: i64
    deposits: i64
}

const ACCOUNT: &str = "bank.Account"

fn account_new(cx: &mut lua.Cx) -> i32!lua.Throw {
    let initial = cx.opt_integer(1, 0)?
    let block = cx.new_userdata(mem.size_of::<Account>(), 0)?
    unsafe {
        *(block as *mut Account) = Account { balance: initial, deposits: 0 }
    }
    cx.set_metatable_by_name(ACCOUNT)?
    return ok(1)
}

fn check_account(cx: &mut lua.Cx) -> (*mut Account)!lua.Throw {
    let block = cx.check_udata(1, ACCOUNT)?
    unsafe {
        return ok(block as *mut Account)
    }
}

fn account_deposit(cx: &mut lua.Cx) -> i32!lua.Throw {
    let a = check_account(cx)?
    let amount = cx.check_integer(2)?
    cx.arg_check(amount > 0, 2, "amount must be positive")?
    unsafe {
        (*a).balance += amount
        (*a).deposits += 1
    }
    cx.push_value(1)
    return ok(1)
}

fn account_balance(cx: &mut lua.Cx) -> i32!lua.Throw {
    let a = check_account(cx)?
    unsafe {
        cx.push_integer((*a).balance)
    }
    return ok(1)
}

fn account_tostring(cx: &mut lua.Cx) -> i32!lua.Throw {
    let a = check_account(cx)?
    b := lua.Buffer.new(cx)
    b.add_str("Account(")?
    unsafe {
        b.add_integer((*a).balance)?
    }
    b.add_str(")")?
    b.push_result(cx)?
    return ok(1)
}

fn open_bank(cx: &mut lua.Cx) -> i32!lua.Throw {
    // The metatable: methods are found through __index.
    cx.new_metatable(ACCOUNT)?
    let methods = [("deposit", account_deposit), ("balance", account_balance), ("__tostring", account_tostring)]
    cx.set_funcs(&methods, 0)?
    cx.push_value(-1)
    cx.set_field(-2, "__index")?
    cx.pop(1)
    let funcs = [("new", account_new)]
    cx.new_lib(&funcs)?
    return ok(1)
}

@test
fn userdata_and_modules() {
    must(userdata_run())
}

fn userdata_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    state.preload_module("bank", open_bank)?
    cx := state.cx()
    let script = b"local bank = require 'bank'\nlocal acct = bank.new(10):deposit(5):deposit(7)\nassert(acct:balance() == 22)\nassert(tostring(acct) == 'Account(22)')\nassert(not pcall(acct.deposit, acct, -1))\nassert(select(2, pcall(acct.balance, {})):find('bank.Account expected, got table', 1, true))\nreturn acct"
    assert_eq(cx.do_string(script), lua.OK)
    let block = cx.check_udata(-1, ACCOUNT)?
    unsafe {
        assert_eq((*(block as *mut Account)).deposits, 2i64)
    }
    return ok()
}

// --- coroutines ----------------------------------------------------------

@test
fn coroutines() {
    must(coroutines_run())
}

fn coroutines_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    cx := state.cx()
    assert_eq(cx.do_string(b"function producer(n) for i = 1, n do coroutine.yield(i * i) end return 'done' end"), lua.OK)
    co := cx.new_thread()?
    co.get_global("producer")?
    co.push_integer(3)
    sum := 0i64
    nres := 0i32
    nargs := 1i32
    for {
        let status = cx.resume(&mut co, nargs, &mut nres)
        if status == lua.YIELD {
            sum += int_at(&co, -1)
            co.pop(nres)
            nargs = 0
        } else {
            assert_eq(status, lua.OK)
            assert(top_is(&mut co, -1, "done"))
            break
        }
    }
    assert_eq(sum, 14i64)
    return ok()
}

// --- limits --------------------------------------------------------------

static mut WARNINGS: i32 = 0

fn count_warning(cx: &mut lua.Cx, msg: &[u8], tocont: bool) {
    if !tocont {
        unsafe {
            WARNINGS += 1
        }
    }
}

fn out_of_budget(cx: &mut lua.Cx, ar: &mut lua.Debug) -> void!lua.Throw {
    return err(cx.error_msg("instruction budget exhausted"))
}

@test
fn limits() {
    must(limits_run())
}

fn limits_run() -> void!lua.Throw {
    state := lua.Lua.new()?
    state.open_libs()?
    state.set_warn_fn(count_warning)
    cx := state.cx()
    assert_eq(cx.do_string(b"warn('disk ', 'almost full'); warn('again')"), lua.OK)
    unsafe {
        assert_eq(WARNINGS, 2)
    }

    cx.set_hook(out_of_budget, lua.MASKCOUNT, 100000)
    assert_eq(cx.do_string(b"while true do end"), lua.ERRRUN)
    assert(top_is(&mut cx, -1, "instruction budget exhausted"))
    cx.pop(1)
    cx.clear_hook()

    let in_use = state.memory_in_use()
    state.set_memory_limit(in_use + 1000000)
    assert_eq(cx.do_string(b"local t = {} for i = 1, 1e8 do t[i] = ('x'):rep(100) .. i end"), lua.ERRMEM)
    assert(top_is(&mut cx, -1, "not enough memory"))
    cx.set_top(0)
    state.set_memory_limit(0)
    assert_eq(state.close(), 0usize)
    return ok()
}
```

## The standalone interpreter

`examples/lua` is a drop-in equivalent of the reference `lua` program. Build
it with optimizations from a source checkout:

```sh
dodo build examples/lua -O 2 -o build/lua
build/lua script.lua arg1 arg2
build/lua -e 'print(_VERSION)'
build/lua -i
```

It supports `-e stat`, `-l mod`, `-l g=mod`, `-i`, `-v`, `-E`, `-W`, `-` and
`--`, the global `arg` table, `LUA_INIT_5_4` and `LUA_INIT`, binary chunks and
scripts that start with a `#!` line. Errors are reported on standard error as
`lua: message` followed by a traceback, with exit status 1, and Ctrl-C
interrupts a running script. The interactive mode reads lines from standard
input without line editing; it evaluates expressions (`> 1 + 1` prints `2`),
continues incomplete statements on a `>>` prompt, and uses `_PROMPT` and
`_PROMPT2`. The program is also a compact example of the embedding API: it
loads and runs chunks through `pcall` with a traceback handler, installs the
`arg` table, and maps statuses to messages.

`tests/lua` holds a corpus of scripts with the output recorded from the
reference interpreter; `cargo test --test lua_corpus` builds the interpreter
and checks every script. `python3 scripts/test_lua.py` also runs the official
Lua 5.4.9 test suite and the benchmarks against a reference build (see
`tests/lua/README.md`).

## Compatibility

The implementation follows the Lua 5.4 reference manual and the observable
behaviour of PUC Lua 5.4.9, including error messages (with variable names such
as `attempt to call a nil value (global 'f')`), `tostring` and
`string.format` output, the order of `next`, the length operator on tables,
and garbage-collector parameters. It passes the official Lua 5.4.9 test suite
except the parts that need the internal test library.

- It matches a default reference build, which defines `LUA_COMPAT_5_3`:
  `math.pow`, `math.ldexp`, `math.frexp`, `math.cosh`, `math.sinh`,
  `math.tanh` and `math.log10` exist, and `__le` falls back to `not __lt`
  with the operands swapped.
- Binary chunks use the Lua 5.4 format for 64-bit integers and doubles:
  chunks from the reference `luac` 5.4 load, and `string.dump` output loads in
  the reference interpreter.
- There is no C API: C modules (`.so`, `.dll`) and `package.loadlib` are not
  available, as in a reference build without dynamic-library support. Write
  native modules in Dodo and register them with `preload_module`.
- The internal test library `T` (an `ltests` build of the reference) is not
  provided.
- `io` and `os` wrap the C library of the target, like the reference, so their
  results follow that system; see [platforms](#platforms).

The Dodo API mirrors the C API but is not identical: functions that can raise
return `T!lua.Throw` instead of unwinding, strings are byte slices with
lengths, and auxiliary-library functions are methods on `Cx`
(`cx.check_integer`, `cx.new_metatable`, `cx.traceback`, ...).

## Platforms

`std/script/lua` supports the hosted targets: x86-64 and AArch64 Linux with
the GNU C library, macOS on Apple silicon and Intel, and x86-64 Windows (GNU
and MSVC toolchains). It links the C library and, on Linux, `libm`. The
language, the compiler and the `string`, `table`, `utf8`, `coroutine`,
`math` and `debug` libraries behave the same everywhere. The `io` and `os`
libraries wrap the C library, as the reference interpreter does:

- `io.stdin`, `io.stdout` and `io.stderr` are the C library's standard
  streams; `print` writes to the same stdout, so output stays ordered, and
  stderr is unbuffered. Dodo code that writes to standard output while Lua
  runs should use `lua.write_stdout` (or flush with `lua.flush_stdout()`) to
  keep its output in order with `print`.
- `io.popen` and `os.execute` run commands through the system shell
  (`/bin/sh` on Linux and macOS, `cmd.exe` on Windows). On Linux and macOS a
  process killed by a signal reports `nil, "signal", signo`; on Windows the
  status is always the exit code. Pending output is flushed before a command
  starts.
- `os.date` accepts the strftime conversions of ISO C99 and POSIX (with the
  `E` and `O` modifiers) on Linux and macOS, and the conversions of the
  Windows C runtime (with the `#` flag) on Windows; others raise
  `invalid conversion specifier`. `os.time`, `os.clock` and `os.getenv` use
  the C library.
- `os.tmpname` creates the file (`/tmp/lua_XXXXXX`) on Linux and macOS, so
  the name cannot be taken by another process; on Windows it only returns a
  name. File positions are 64-bit everywhere.
- `os.setlocale` changes the process locale; the decimal point of the
  current numeric locale is honoured when numbers are converted, as in the
  reference.
- Error results use the C library's messages and errno values, for example
  `nil, "x.txt: No such file or directory", 2`; the wording differs between
  systems.
- On Windows, files opened in text mode and the standard streams translate
  line endings (`\n` is written as CRLF, and a CR before LF is dropped on
  reading), and `package.config` uses `\` as the directory separator, so
  `require "a.b"` looks for `a\b.lua`; all as in the reference built for
  Windows.
- `string.format` with `%a` and the printing of very small (subnormal)
  floats follow the platform's C library.
- Addresses in `tostring` results and `string.format("%p")` always have the
  form `0x` followed by lowercase hex digits; the reference on Windows prints
  the C runtime's form without `0x`. Programs should not depend on either.

The per-system C differences live in the `std/script/native` adapters, which
resolve to `std/script/posix` or `std/script/windows` for the compilation
target, with a small C runtime compiled against the target's headers.

## Performance

The interpreter is written for optimized builds: build programs that embed
Lua with `-O 2` (or `dodo build --release`). An unoptimized build runs Lua
code several times slower and is meant for debugging. On the benchmark set in
`benchmarks/lua`, an `-O 2` build of `examples/lua` ran at a geometric mean of
0.82 times the time of the reference PUC Lua 5.4.9 built with its default
`-O2` (Apple M-series, macOS, at the time of writing): from 0.5× on
`coroutine_ring` and 0.6× on `table_ops` to about 1.0× on `mandelbrot` and
`table_append`, the only benchmarks not faster than the reference. Run `python3 scripts/test_lua.py bench` to measure on your machine.

Calls between Dodo and Lua are direct function calls with no marshalling
layer. Keep hot loops on one side of the boundary where possible, reuse
tables instead of recreating them, and prefer `raw_get_i`/`raw_set_i` for
array access from Dodo when no metamethods are involved.
