// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe %t.o
// RUN: readelf -a %t.exe

        .weak   weak_fn
        .global _start
_start:
        nop
