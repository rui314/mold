// RUN: cc -o %t1.o -c %s
// RUN: cc -shared -fPIC -o %t2.so -xc - < /dev/null
// RUN: mold -o %t.exe %t1.o %t2.so --export-dynamic
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
