// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe %t.o -rpath /foo -rpath /bar
// RUN: readelf --dynamic %t.exe | FileCheck %s
// CHECK: 0x000000000000001d (RUNPATH) Library runpath: [/foo]
// CHECK: 0x000000000000001d (RUNPATH) Library runpath: [/bar]

        .text
        .globl _start
_start:
        nop
