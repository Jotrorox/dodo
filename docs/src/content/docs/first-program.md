---
title: "Your first program"
description: "Write and run a tiny Dodo program, then check it and build an executable."
section: "Start here"
order: 20
---

You need [Dodo and a C toolchain installed](installation.md). This guide uses a
terminal and a text editor.

## 1. Create a file

Save this code in a file named `hello.dodo`:

```dodo
package hello

fn main() -> i32 {
    return 0
}
```

- `package hello` names the package that contains this file.
- `fn main()` defines the function that runs when the program starts.
- `-> i32` says that `main` returns a 32-bit integer.
- `return 0` ends the program with a success exit status.

Braces surround the function body. Each statement can end at a newline, so this
program needs no semicolons.

## 2. Run it

Open a terminal in the directory containing `hello.dodo` and run:

```sh
dodo run hello.dodo
```

The program prints nothing. Your terminal returns to its prompt when the
program finishes successfully. Returning `0` sets the exit status; it does not
print the number.

On a Linux shell, you can see that status by running this immediately afterward:

```sh
echo $?
```

It prints `0`.

## 3. Check and build

To check the code without running it:

```sh
dodo check hello.dodo
```

A successful check prints `Checked hello.dodo`. If the code has an error, Dodo
reports where it occurred.

`run` creates a temporary executable. To keep an executable and run it yourself:

```sh
dodo build hello.dodo -o build/hello
./build/hello
```

The build prints `Built build/hello`. Running `./build/hello` then runs the same
program, with the same empty output and success status.

## Next steps

Learn [everyday compiler commands](command-line.md), including `dodo fmt` for
formatting your code, or set up [your editor](editors.md).
