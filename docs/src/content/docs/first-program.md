---
title: "Your first program"
description: "Create, run, change, check, and build a complete Dodo program, with each step explained."
section: "Start here"
order: 20
---

By the end of this page, you will have a program that prints a greeting and an
executable you can run yourself. You need a text editor, a terminal, and
[Dodo with a C toolchain installed](installation.md). No prior knowledge of
borrowing or systems programming is required.

Commands go in your terminal. Dodo code goes in a `.dodo` file. The examples use
Linux shell commands unless a PowerShell version is shown; `dodo` commands work
in either shell.

## 1. Create a project folder

Create an empty folder and enter it:

```sh
mkdir hello
cd hello
```

Save the following as `main.dodo` inside `hello`. Check that your editor has not
added `.txt` to the filename. A project needs no manifest or package manager.

```dodo test
package main

import "std/console"

fn main() {
    console.println("Hello, world!")!
}
```

Your folder now contains:

```text
hello/
  main.dodo
```

## 2. Run it

From the terminal in `hello`, run:

```sh
dodo run
```

Expected output:

```text
Hello, world!
```

The compiler first checks your source, creates a temporary executable, and runs
it. When the program finishes, you return to the terminal prompt. There is no
separate install step for `std/console`: it is bundled with Dodo.

If the terminal cannot find `dodo`, revisit [installation](installation.md).
If the compiler cannot execute `cc`, configure the C toolchain. If it cannot
find `main.dodo`, open the terminal in the folder containing your source.

## 3. Understand each line

| Code | What it means |
| --- | --- |
| `package main` | Names the package this file belongs to. Every source file begins with a package declaration. |
| `import "std/console"` | Makes console input and output available under the name `console`. |
| `fn main()` | Defines the entry function called when your executable starts. It takes no arguments. |
| `{ ... }` | Groups the statements in a function body. |
| `console.println(...)` | Writes a value and then a newline to standard output. |
| `"Hello, world!"` | An immutable UTF-8 string literal stored with the program. |
| The final `!` | Takes the successful value from a Result, or panics if the operation failed. |

Printing can fail, for example when an output pipe is closed. Its Result reports
either the byte count or an I/O error. The final `!` explicitly chooses to stop
on an error for this first example. Later, you will use
[`match` and `?`](patterns-and-results.md) to recover or pass an error to a caller.
The exclamation mark *inside* the greeting is just printed text.

A newline ends each statement here; semicolons are optional. Reaching the end of
this `main` returns success, exit status `0`. That status is not printed. To see
it immediately after a run, use `echo $?` on Linux or `$LASTEXITCODE` in PowerShell.

## 4. Change the program

Replace the complete contents of `main.dodo` with:

```dodo test
package main

import "std/console"

fn main() {
    let name = "Dodo"
    let lessons = 3i32
    console.printf("Hello, {}!\n", name)!
    console.printf("{} lessons to try.\n", lessons)!
}
```

Run `dodo run` again. It prints:

```text
Hello, Dodo!
3 lessons to try.
```

`let` gives a value a name without allowing reassignment. `3i32` is the integer
3 with a signed 32-bit type. Each `{}` in a literal format is replaced by the
corresponding argument. `\n` is a newline; `printf` adds only the characters you
request. The compiler checks the literal format and its arguments.

**Try it:** change the name and lesson count, then run again. Remove the second
argument from one `printf` call and run `dodo check` to see the diagnostic.
Restore the argument before continuing.

<span id="3-check-and-build"></span>

## 5. Check and format

These commands do different jobs:

```sh
dodo check
dodo fmt
dodo check
```

`check` verifies syntax, types, ownership, and borrowing without running the
program or invoking the linker. A successful check prints `Checked main.dodo`.
`fmt` rewrites source with consistent formatting. Save your editor changes before
running commands so they use the current file.

A successful check cannot predict every runtime failure. Index bounds, integer
overflow, and failures reported by library calls still matter when a program runs.

## 6. Keep an executable

`run` removes its temporary executable afterward. Use `compile` when you want a
file to keep. On Linux:

```sh
dodo compile
./build/hello
```

The default name comes from the project folder. On Windows, choose an explicit
`.exe` path and run it with PowerShell:

```powershell
dodo compile -o build/hello.exe
.\build\hello.exe
```

Both commands run your program again. You do not need Dodo to launch the resulting
executable, but its platform runtime and any libraries you linked must be
available. The [command-line guide](command-line.md) explains output names,
optimization, debugging, and target selection.

## Next steps

Continue with [variables, values, and expressions](language-basics.md), then
[types and functions](types-and-functions.md). The
[temperature report walkthrough](practical-program.md) combines these ideas in
one complete program. [Editor setup](editors.md) adds diagnostics and completion
as you type; [console I/O](console.md) extends printing to input and stderr.
