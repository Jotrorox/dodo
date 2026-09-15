---
title: "Your first program"
description: "Write and run a tiny Dodo program, then check it and build an executable."
section: "Start here"
order: 20
---

You need [Dodo and a C toolchain installed](installation.md). This guide uses a
terminal and a text editor.

## 1. Create a project folder

Create a folder for your program and open it:

```sh
mkdir hello
cd hello
```

Save this code as `main.dodo` in that folder. No manifest, lockfile, or package
manager is needed:

```dodo test
package main

import "std/console"

fn main() {
    console.println("Hello, world!")!
}
```

- `package main` names the package that contains this file.
- `import "std/console"` loads safe hosted console I/O from the bundled library.
- `fn main()` defines the function that runs when the program starts.
- `console.println("Hello, world!")` writes the greeting followed by a newline.
- Postfix `!` handles the Result: it unwraps the written byte count on success
  and panics if printing fails. Use `?` in a Result-returning function to
  propagate an error, or `match` to recover from it.
- Reaching the end of `main` sets a success exit status.

Braces surround the function body. Each statement can end at a newline, so this
program needs no semicolons.

## 2. Run it

Open a terminal in the directory containing `main.dodo` and run:

```sh
dodo run
```

`dodo run` looks for `main.dodo` in the current folder.
The program prints:

```text
Hello, world!
```

Your terminal returns to its prompt when the program finishes. Completing
`main` sets exit status `0`; it does not print the number. Console access supports
Linux GNU x86-64 and Windows x64 and needs no unsafe code in your program.

On a Linux shell, you can see that status by running this immediately afterward:

```sh
echo $?
```

It prints `0`.

## 3. Check and build

To check the code without running it:

```sh
dodo check
```

A successful check prints `Checked main.dodo`. If the code has an error, Dodo
reports where it occurred.

`run` creates a temporary executable. To keep an executable and run it yourself:

```sh
dodo compile
./build/hello
```

The output takes its name from the project folder, `hello`.
The build prints `Built build/hello`. Running `./build/hello` then runs the same
program, with the same greeting and success status.

## Next steps

Continue with [console I/O](console.md) to print values, report errors to stderr,
and read a line into caller-owned storage.

Learn [everyday compiler commands](command-line.md), including `dodo fmt` for
formatting your code, organize shared code in
[ordinary subfolders](packages.md), or set up [your editor](editors.md).
