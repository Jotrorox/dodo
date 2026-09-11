// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See LICENSE.compiler-rt in this directory for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
// Adapted from LLVM compiler-rt 22.1.0:
// https://github.com/llvm/llvm-project/blob/llvmorg-22.1.0/compiler-rt/lib/builtins/x86_64/chkstk.S
// Changes: expand the COFF declaration macros locally, export the MSVC x64
// name __chkstk, and use this standalone file only in Windows test executables.
// The probe instructions are unchanged. No CRT allocation or startup is needed.
//
// RAX supplies the requested frame size. Probe every intervening 4 KiB page
// so Windows can commit its stack guard pages. Preserve RAX and RCX; the caller
// adjusts RSP after this function returns.

#if !defined(__x86_64__) || !defined(_WIN32)
#error "windows_chkstk.S is only for Windows x86-64 test executables"
#endif

.text
.balign 4
.globl __chkstk
.def __chkstk
.scl 2
.type 32
.endef
__chkstk:
        push   %rcx
        push   %rax
        cmp    $0x1000,%rax
        lea    24(%rsp),%rcx
        jb     1f
2:
        sub    $0x1000,%rcx
        test   %rcx,(%rcx)
        sub    $0x1000,%rax
        cmp    $0x1000,%rax
        ja     2b
1:
        sub    %rax,%rcx
        test   %rcx,(%rcx)
        pop    %rax
        pop    %rcx
        ret
