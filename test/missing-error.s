// RUN: cc -o %t.o -c %s
// RUN: not mold -o %t.exe %t.o 2> %t.log
// RUN: grep 'undefined symbol: .*\.o: foo' %t.log

        .text
        .globl main
main:
        call foo
