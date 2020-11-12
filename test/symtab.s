// RUN: cc -o %t1.o -c %s
// RUN: echo '.globl this_is_global; local2: this_is_global:' | \
// RUN:   cc -x assembler -o %t2.o -c -
// RUN: mold -o %t.exe %t1.o %t2.o
// RUN: readelf --symbols %t.exe | FileCheck %s

// CHECK: foo

        .globl foo, bar, this_is_global
local1:
foo:
bar:
        .byte 0
