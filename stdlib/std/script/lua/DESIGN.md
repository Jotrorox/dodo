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
| `api.dodo`, `aux.dodo` | Public embedding API. |
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
