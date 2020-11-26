// RUN: cc -o %t.o -c %s
// RUN: not mold -o %t.exe %t.o %t.o 2> %t.log
// RUN: grep 'duplicate symbol: .*\.o: .*\.o: main' %t.log

        .text
        .globl main
main:
        nop
