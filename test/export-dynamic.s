// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe %t.o --export-dynamic
// RUN: readelf --dyn-syms %t.exe | FileCheck %s

        .text
        .globl foo
        .hidden foo
foo:
        nop
        .globl bar
bar:
        nop
        .globl _start
_start:
        nop
