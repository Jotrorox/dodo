# Lua for Dodo: internal design

`std/script/lua` is a complete implementation of Lua 5.4 (reference: the Lua
5.4 Reference Manual and the observable behaviour of PUC Lua 5.4.9) written in
Dodo. Other embeddable languages may later live beside it under `std/script/`.

All code in this package is original. It is **not** a translation of the PUC
Lua C sources. We implement documented *interfaces* of Lua 5.4 — the language,
the standard library, the virtual-machine instruction set and the binary-chunk
format (so `luac` 5.4 output loads and our `string.dump` output loads in PUC
Lua) — but every function, data structure and comment is written fresh. Do not
copy or line-by-line translate code from the PUC `.c` files. Opcode semantics
(below) and file formats are specifications, not code.

## Files and ownership

One directory package, `package lua`. Every file shares one namespace, so
private helpers use a file-specific prefix (`tab_`, `gc_`, `vm_`, `lex_`, ...).

| File | Contents |
| --- | --- |
| `object.dodo` | Shared object model: tags, `Value`, `GCObject`, `LString`, `Proto` and its debug records, `Throw`. |
| `opcodes.dodo` | Instruction layout, opcode numbers, metamethod event numbers. |
| `mem.dodo` | `mem_alloc`, `mem_realloc`, `mem_free` (GC accounting). |
| `string.dodo` | String creation and interning (`str_new`, `str_from`, `str_equal`). |
| `table.dodo` | Tables. |
| `func.dodo` | Prototypes, closures, upvalues. |
| `gc.dodo` | Garbage collector. |
| `state.dodo` | `Global`, `State` (thread), `CallInfo`, stack management. |
| `errors.dodo` | Raising errors (`err_syntax`, runtime error helpers). |
| `call.dodo`, `vm.dodo`, `meta.dodo` | Calls/returns/coroutines, the interpreter loop, metamethods. |
| `lex.dodo`, `parse.dodo`, `code.dodo` | Compiler: `compile(...)`. |
| `dump.dodo`, `undump.dodo` | Binary chunks. |
| `number.dodo` | Number formatting and parsing (`num_*`). |
| `pattern.dodo` | Pattern matching engine (`pat_*`). |
| `api.dodo`, `auxlib.dodo` | Public embedding API. |
| `lib_*.dodo` | Standard libraries. |

## Coding conventions

* Internals work on raw pointers inside `unsafe { }`, like C: `L: *mut State`,
  `*mut Value`, `*mut Table`. Use `*mut` everywhere (Dodo has no implicit
  `*mut T` → `*const T` coercion, so mixing them only adds casts).
* Dodo cannot take `&mut (*p).field` of a raw pointer. Address a field with
  `ptr.offset(p as *mut u8, mem.offset_of::<T>("field") as isize) as *mut F`.
  Wrap common ones in small helper functions.
* **Never name a struct field `len`**: `(*p).len` is miscompiled as a slice
  length ("is not an array or slice"). Use `length`, `size`, `count`.
* Integer arithmetic traps on overflow: Lua integer arithmetic must use
  `core.wrapping_add/sub/mul`. `as` conversions are checked and trap: mask
  before narrowing, range-check floats before converting (`f as i64` traps on
  NaN or out of range). Shifts trap for counts >= width. Reinterpret
  `u64`↔`i64`↔`f64` bits through pointers (`*(p as *mut i64)`), not `as`.
* `&` binds looser than `==`: write `(x & 1) != 0`.
* Structs and enums always move; keep hot data in scalars and raw pointers.
* Everything that can raise a Lua error returns `T!Throw` and propagates with
  `?`. There is no unwinding: a raised error is an ordinary returned `err`.
* Performance: the interpreter must be fast at `-O2`. Avoid checked slices in
  hot paths (use raw pointers), avoid per-operation allocation, keep the
  `Value` 16 bytes, keep fast paths branch-light.

## Values

`Value { bits: u64, tag: u8 }` (16 bytes). Tags (see object.dodo): low nibble
is the basic type (0 nil … 8 thread), bits 4–5 the variant (integer vs float,
short vs long string, Lua vs native closure, nil vs empty slot vs absent key),
bit 6 marks collectable values whose `bits` hold an object pointer.

Collectable objects start with the `GCObject` header (`next`, `tag`,
`marked`). Strings are `LString` followed inline by their bytes and a NUL.
Short strings (≤ 40 bytes) are interned: equal short strings are the same
object, so they compare by pointer. Long strings compare by content.

## Errors

To raise, store the error object in `L.errobj` and return `err(Throw.X)`:
`Runtime` (LUA_ERRRUN), `Syntax`, `Memory`, `Handler` (LUA_ERRERR). Protected
calls (pcall, coroutine resume, API pcall) catch the `err`, read `L.errobj`,
close pending to-be-closed variables and upvalues above the protected level,
and restore the call stack. `Throw.Yield` is not an error: it unwinds native
frames when a coroutine yields across them (see "Coroutines").

Error-message formats must match PUC Lua exactly, including the `source:line:`
position prefix added by `error` level 1 and by runtime errors, and variable
descriptions such as `attempt to index a nil value (local 'x')`, `(global 'f')`,
`(field 'y')`, `(upvalue 'u')`, `(method 'm')`, `(constant 'c')`.

## Garbage collection contract

* The collector runs only at explicit check points (`gc_check(L)`), placed where
  every live object is reachable from the roots (registry, main thread, every
  thread's stack up to its top, open upvalues, metatables of basic types,
  `L.errobj`). A raw object pointer held only in a Dodo local is not a root, so
  code that creates several objects anchors them on the Lua stack (or in a
  reachable table) before the next check point.
* `compile(...)` runs with collection blocked (the loader blocks it); the
  compiler therefore needs no barriers or anchoring.
* Writes of a collectable value into an object that may already be black need
  a barrier: `gc_barrier(L, parent, value)` (forward) or `gc_barrier_back(L,
  table)` for tables.

## Compiler interface

```
fn compile(L: *mut State, source: *mut u8, length: usize, chunkname: *mut LString) -> (*mut Proto)!Throw
```

Compiles a complete text chunk (the loader concatenates reader pieces and
detects binary chunks, which start with `\x1bLua`). The main prototype is
vararg (`is_vararg = 1`, begins with `VARARGPREP 0`) and has exactly one
upvalue, `_ENV`, with `instack = 1`, `index = 0`. Syntax errors use
`err_syntax` with the PUC message format, e.g.
`[string "x = "]:1: unexpected symbol near <eof>`,
`input:3: '=' expected near 'y'`, `chunk has too many syntax levels`.
The chunkname rules (`=name`, `@file`, otherwise `[string "first line..."]`)
are those of `luaO_chunkid` in PUC Lua: implement them in one shared helper
(`chunk_id`) used by both the compiler and the runtime.

## Instruction set

The Lua 5.4 instruction set, with the layout in opcodes.dodo. `R[x]` is a
register, `K[x]` a constant, `U[x]` an upvalue, `RK(C)` is `K[C]` if `k` is set
else `R[C]`, `sB`/`sC`/`sBx`/`sJ` are signed operands. A comparison or test
instruction is always followed by a `JMP`; "skip" means skip that jump.

| Opcode | Semantics |
| --- | --- |
| MOVE A B | R[A] := R[B] |
| LOADI A sBx | R[A] := integer sBx |
| LOADF A sBx | R[A] := float sBx |
| LOADK A Bx | R[A] := K[Bx] |
| LOADKX A | R[A] := K[Ax of the next EXTRAARG] |
| LOADFALSE A / LOADTRUE A | R[A] := false / true |
| LFALSESKIP A | R[A] := false; pc++ |
| LOADNIL A B | R[A .. A+B] := nil |
| GETUPVAL A B / SETUPVAL A B | R[A] := U[B] / U[B] := R[A] |
| GETTABUP A B C | R[A] := U[B][K[C]] (K[C] a short string) |
| GETTABLE A B C | R[A] := R[B][R[C]] |
| GETI A B C | R[A] := R[B][C] (integer C) |
| GETFIELD A B C | R[A] := R[B][K[C]] (short string) |
| SETTABUP A B C | U[A][K[B]] := RK(C) |
| SETTABLE A B C | R[A][R[B]] := RK(C) |
| SETI A B C | R[A][B] := RK(C) |
| SETFIELD A B C | R[A][K[B]] := RK(C) |
| NEWTABLE A B C k | R[A] := {}; B is log2(hash size)+1 (0 = none); array size is C, plus `Ax*(MAXARG_C+1)` of the following EXTRAARG when k is set. Always followed by EXTRAARG. |
| SELF A B C | R[A+1] := R[B]; R[A] := R[B][RK(C)] |
| ADDI A B sC | R[A] := R[B] + sC |
| ADDK…BXORK A B C | R[A] := R[B] op K[C] (K[C] a number) |
| SHRI A B sC / SHLI A B sC | R[A] := R[B] >> sC / R[A] := sC << R[B] |
| ADD…SHR A B C | R[A] := R[B] op R[C] |
| MMBIN A B C | call metamethod C (TM_* event) over R[A], R[B]; result into the destination of the preceding arithmetic instruction |
| MMBINI A sB C k | same with immediate sB (k: operands swapped) |
| MMBINK A B C k | same with K[B] (k: operands swapped) |
| UNM, BNOT, NOT, LEN A B | R[A] := op R[B] |
| CONCAT A B | R[A] := R[A] .. … .. R[A+B-1] |
| CLOSE A | close upvalues and to-be-closed variables >= R[A] |
| TBC A | mark R[A] as a to-be-closed variable |
| JMP sJ | pc += sJ |
| EQ A B k | if ((R[A] == R[B]) ~= k) then pc++ |
| LT, LE A B k | same with < and <= |
| EQK A B k | if ((R[A] == K[B]) ~= k) then pc++ |
| EQI, LTI, LEI, GTI, GEI A sB k | compare R[A] with immediate sB (C holds an "is float" flag used for error messages) |
| TEST A k | if (not R[A] == k) then pc++ |
| TESTSET A B k | if (not R[B] == k) then pc++ else R[A] := R[B] |
| CALL A B C | R[A], …, R[A+C-2] := R[A](R[A+1], …, R[A+B-1]); B = 0: args up to top; C = 0: results set top |
| TAILCALL A B C k | return R[A](R[A+1], …, R[A+B-1]); k: close upvalues first; C > 0: vararg function with C-1 fixed params |
| RETURN A B C k | return R[A], …, R[A+B-2]; B = 0: up to top; k: close upvalues/tbc first; C > 0: vararg function (C-1 = numparams) |
| RETURN0 / RETURN1 A | return / return R[A] |
| FORPREP A Bx | prepare numeric loop over R[A] (init), R[A+1] (limit), R[A+2] (step), control in R[A+3]; skip loop (pc += Bx + 1) if it does not run |
| FORLOOP A Bx | update counters; if the loop continues, pc -= Bx |
| TFORPREP A Bx | create to-be-closed R[A+3]; pc += Bx |
| TFORCALL A C | R[A+4], …, R[A+3+C] := R[A](R[A+1], R[A+2]) |
| TFORLOOP A Bx | if R[A+4] ~= nil then { R[A+2] := R[A+4]; pc -= Bx } |
| SETLIST A B C k | R[A][C+i] := R[A+i], 1 <= i <= B (B = 0: up to top; k: C += Ax of the next EXTRAARG * (MAXARG_C+1)) |
| CLOSURE A Bx | R[A] := closure(P[Bx]) |
| VARARG A C | R[A], …, R[A+C-2] = vararg (C = 0: all, sets top) |
| VARARGPREP A | adjust vararg parameters (A = number of fixed params) |
| EXTRAARG Ax | extra argument of the previous opcode |

Numeric `for` semantics follow the manual: the loop count for integer loops is
precomputed (no overflow), float loops use floats, a zero step raises
`'for' step is zero`, and non-numeric values raise `'for' initial value must be
a number` (and `limit`, `step`).

## Line information

Matches the 5.4 binary format. `lineinfo[pc]` is a signed byte holding the line
difference from the previous instruction (the first instruction's base is
`linedefined`). If a difference does not fit in [-127, 127], or after 128
consecutive relative entries, `lineinfo[pc]` is -128 (`ABSLINEINFO`) and an
`AbsLineInfo { pc, line }` entry gives the absolute line. To find the line of
`pc`, start from the last `abslineinfo` entry with `pc <= target` (or from
`linedefined` at pc -1) and add relative entries forward.

## Binary chunks

Header: `\x1bLua`, version byte `0x54`, format `0`, `\x19\x93\r\n\x1a\n`,
instruction size 4, integer size 8, number size 8, the integer `0x5678`, the
float `370.5`, then the number of upvalues of the main function (one byte) and
the main function. Sizes are written as variable-length unsigned integers
(7 bits per byte, most significant group first, the last byte has bit 7 set).
`string.dump(f, strip)` and `load` must round-trip with PUC Lua 5.4.

## Runtime notes for library authors

The standard libraries (`lib_*.dodo`) are native functions written against
the public API in api.dodo/auxlib.dodo (`Cx`, `Buffer`, `LuaStr`), which mirrors
the C API (`cx.check_integer` ~ `luaL_checkinteger`, `cx.get_field` ~
`lua_getfield`, ...). Being in the same package they may also use internal
functions for hot paths (`tab_get_int`, `tab_set_int`, `str_new`, ...), with
the collector rules above.

* Signature: `fn name(cx: &mut Cx) -> i32!Throw`; return the number of
  results pushed. Raise with `return err(cx.error_msg("..."))` (adds the
  caller's position like `luaL_error`), `cx.arg_error(n, "...")`,
  `cx.type_error(n, "tname")` or `return err(cx.error())` for the value on
  top. Argument errors name the function from the call site, else from
  `package.loaded` (`'table.sort'`), else `?`.
* Strings: `cx.check_lstr(n)` / `cx.to_lstr(n)` return a `LuaStr` view whose
  bytes stay valid while the value stays on the stack; it does not borrow
  `cx`. Build results with `Buffer` (Dodo-owned, freed on drop) and
  `buf.push_result(cx)`.
* Registration: `cx.new_lib(&[("name", f), ...])` or `cx.set_funcs(...)`
  (with upvalues); `open_all_libs` (auxlib.dodo) calls `require_lib` for each
  library — add new libraries there.
* String metatable: lib_string creates it (`__index = string`) and must call
  `set_string_arith_metamethods(cx)` on it: 5.4 converts strings for
  arithmetic through those metamethods, not in the core.
* Streams: `c_stdin()/c_stdout()/c_stderr()` are the C library's own
  `stdin`/`stdout`/`stderr`, shared with `print`, warnings and io.stdin/
  stdout/stderr so output stays ordered; `last_os_error_message()`,
  `cx.file_result(...)`, `cx.exec_result(...)` follow
  luaL_fileresult/luaL_execresult (errno is read through the host boundary).
* Host boundary: C library functions whose names, constants or layouts
  differ between Linux, macOS and Windows go through `std/script/native`
  (adapters `std/script/posix.dodo` and `std/script/windows.dodo`, C side in
  `std/script/runtime.c`, compiled against the target headers): errno,
  the standard streams, 64-bit seek/tell, setvbuf modes, popen/pclose/system
  with decoded exit statuses, time/clock/localtime/gmtime/mktime/strftime on
  an opaque `struct tm`, setlocale categories, tmpname and getenv. Declare
  directly (`unsafe extern "C" fn`) only functions with the same name and
  ABI everywhere (fopen, fread, getc, remove, ...), and only once per
  package: every file shares one namespace, so a second declaration of the
  same C function in another `lib_*` file is a duplicate.
* Yields: natives that call Lua and may be interrupted by a yield use
  `cx.call_k` / `cx.pcall_k` with a continuation; plain `cx.call` is
  non-yieldable ("attempt to yield across a C-call boundary").
* `lib_debug` builds on `cx.get_stack`, `cx.get_info` (with `'>'`, `'f'`,
  `'L'`), `cx.get_local`/`set_local`/`get_param_name`, `cx.set_hook` (a
  `HookFn` receiving a `Debug` with `event` and `currentline`),
  `get_upvalue`/`set_upvalue`/`upvalue_id`/`upvalue_join`,
  `get_iuservalue`/`set_iuservalue` and `cx.traceback`.

* `lib_package`: `require`, the searchers and the path lookups take the
  package table as upvalue 1; `package.loaded`/`preload` are the registry
  tables `_LOADED`/`_PRELOAD`. There is no C API, so the C searchers and
  `loadlib` behave like a reference build without dlopen (message
  `dynamic libraries not enabled; check your Lua installation`, error kind
  `absent`). Embedders register Dodo modules with `Cx.preload_module`.
  Names and paths are cut at the first NUL like the C strings of the
  reference (`pkg_cstr`).
* `lib_debug`: Lua hooks live in the registry table `_HOOKKEY` (weak keys,
  indexed by thread) and run through the native hook `db_hook`; an error
  raised in a hook leaves the hooked frame marked `CIST_HOOKED` until the
  catching protected call unwinds it, so message handlers see
  `in hook '?'` as in the reference.
* C library functions declared by a library file use `@link_name` with a
  file-prefixed Dodo name (e.g. `pkg_getenv`, `db_fgets`): a package may
  declare each name only once, and several library files need the same C
  functions.

## Performance notes

Benchmarks live in `benchmarks/lua` (`python3 scripts/test_lua.py bench`).
Things the interpreter's speed depends on:

* **Threaded dispatch.** The compiler gives an interpreter loop one indirect
  branch per opcode handler only if the loop keeps its shape: the inner
  `for { ... }` of `vm_execute` has no condition, its last statement is
  `match ((i & 127) as u8) { ... }` with one arm per opcode, and the
  statements before the match (fetch, trap test, `ra`) are plain `let`s with
  distinct names that shadow nothing, plus assignments. Each arm that falls
  through runs those statements again and dispatches by itself. Moving the
  match, matching on a wider type, or adding a guard or range arm before
  the opcode arms silently falls back to one shared switch.
* **Keep the dispatch path short.** The trap test stays a predicted branch
  before the dispatch; folding it into the opcode (`op | trapbit`) lengthens
  the dependency chain to the indirect branch and was slower.
* **Inlining is explicit.** Hot helpers called from `vm_execute` (table
  lookups, barriers, stack arithmetic) are `@inline`; error, hook,
  metamethod and allocation slow paths are `@cold` or `@inline(never)` so
  they do not bloat the loop or perturb its register allocation. Check the
  calls left in `vm_execute` with `--emit asm` after larger changes.
* **Frame entry.** CALL and the returns set `cl`, `pc` and `base` directly;
  re-deriving them through `ci->func` costs a chain of dependent loads.
* **Checked arithmetic.** Use wrapping operations, masked shift counts and
  `small_i32` for values known to be in range; each remaining check is a
  compare and a branch, and the panic paths make helpers look bigger to the
  inliner.
* **Result payloads.** `T!Throw` with a non-void `T` returns its payload as
  bytes the caller reassembles; hot internal functions return `void!Throw`
  and write results through out-parameters (e.g. `tab_find_index`).
* **Allocation.** Blocks up to 256 bytes come from the small-block free
  lists in mem.dodo; the system allocator's slow paths dominated
  allocation-heavy programs. Frees must pass the exact allocated size
  (`MEM_CHECK` in mem.dodo verifies this; run the corpus and the suite with
  it after changing object layouts).

## Development workflow

* Fast builds: the stdlib is embedded when cargo builds the compiler, so
  instead of rebuilding it after every edit, build a project whose `lua`
  entry is a directory symlink to `stdlib/std/script/lua` and that imports
  `"lua"`: `mkdir /tmp/luadev && ln -s $REPO/stdlib/std/script/lua
  /tmp/luadev/lua`, copy `examples/lua/main.dodo` there with `import "lua"`,
  and build it with a release compiler (`cargo build --release`;
  `target/release/dodo build /tmp/luadev/main.dodo -O 2 -o build/lua` takes
  ~13 s). File symlinks inside a package directory are not followed.
* `python3 scripts/test_lua.py fetch` builds the reference PUC Lua 5.4.9 in
  `target/lua-reference/`; `check --lua build/lua` runs the tests/lua corpus,
  `suite --lua build/lua` the official test suite, `diff` compares against
  the reference, `bench` runs benchmarks/lua.
* GC stress testing: `Lua.set_gc_stress(1)` makes the collector run one
  incremental step at every check point and `set_gc_stress(2)` a full
  collection (the stand-alone interpreter reads `DODO_LUA_GC_STRESS=1|2`).
  Programs must behave the same apart from finalizer timing; a difference
  means a missing barrier or an unanchored object. Run the corpus with
  `DODO_LUA_GC_STRESS=1 python3 scripts/test_lua.py check` (level 2 changes
  when finalizers run, so `meta_gc` legitimately differs there).
* `python3 scripts/lua_fuzz.py run --count N --seed S` diffs randomly
  generated programs (runtime semantics, not syntax) against the reference;
  `--gc-stress 1` runs ours under the stress mode.
* The reference is built with LUA_COMPAT_5_3 (the default makefile setting):
  match it (`math.pow`, `math.ldexp`, ..., `__le` falling back to `not __lt`).
