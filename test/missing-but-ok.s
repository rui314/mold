// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe %t.o

        .text
        .globl _start, foo
_start:
        nop
