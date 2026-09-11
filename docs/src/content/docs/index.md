---
title: "Dodo documentation"
description: "An introduction to Dodo, a small systems language with explicit control and checked borrowing."
order: 0
---

Dodo is a small, ahead-of-time compiled systems language with checked borrowing
and explicit hardware access. Its Rust and LLVM compiler produces native
executables, object files, assembly, LLVM IR, and bitcode. Ordinary generated
code needs no garbage collector, heap allocator, scheduler, or Dodo runtime.

The current compiler release is **0.1.0**. The language specification describes a
broader design; this first release does not claim complete specification
conformance or a proof of memory safety. Read the
[implementation decisions and limits](implementation.md) for current support.

## Start with a small program

Install a built `dodo` binary on your `PATH`, or follow the
[source build instructions](https://github.com/Jotrorox/dodo#build-and-install).
Using a built compiler does not require an LLVM installation. Building and
running a hosted executable requires a C toolchain such as `cc`.

Save this as `hello.dodo`:

```dodo
package hello

unsafe extern "C" fn putchar(character: i32) -> i32

fn main() {
    for &character in b"Dodo 0.1\n" {
        // SAFETY: putchar accepts each promoted unsigned byte.
        unsafe {
            putchar(character as i32)
        }
    }
}
```

Then format, check, and run it:

```sh
dodo fmt hello.dodo
dodo check hello.dodo
dodo run hello.dodo
```

The program prints `Dodo 0.1`. Explore more
[examples in the repository](https://github.com/Jotrorox/dodo/tree/main/examples),
or use `dodo --help` to see compiler commands and output formats.

## Find your way around

- [Language specification](language-spec-0.1.md): syntax, types, ownership,
  hardware boundaries, worked examples, and open design questions.
- [Implementation decisions and limits](implementation.md): what the compiler
  supports today and where the design remains unimplemented.
- [Implementation requirements](spec-requirements.md): a checklist for reviewing
  language behavior and planning conformance tests.
- [Diagnostics and editors](diagnostics-and-editors.md): ownership error examples
  and setup for diagnostics and hovers with `dodo lsp`.
- [Edit these docs](contributing.md): write Markdown, preview locally, and publish
  through GitHub Pages.

## Read offline

Download the language specification as
[plain text](/downloads/language-spec-0.1.txt) or a
[PDF with searchable text and section bookmarks](/downloads/language-spec-0.1.pdf).
Both are generated from the same Markdown source used by this site.
