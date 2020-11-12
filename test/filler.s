// RUN: cc -o %t.o -c %s
// RUN: mold -static --filler 0xfe -o %t1.exe \
// RUN:   /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbeginT.o \
// RUN:   %t.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc_eh.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libc.a \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o
// RUN: mold -static --filler 0x0 -o %t2.exe \
// RUN:   /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbeginT.o \
// RUN:   %t.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc_eh.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libc.a \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o
// RUN: hexdump -C %t1.exe > %t1.txt
// RUN: hexdump -C %t2.exe > %t2.txt
// RUN: diff %t1.txt %t2.txt

        .text
        .globl main
main:
        lea msg(%rip), %rdi
        xor %rax, %rax
        call printf
        xor %rax, %rax
        ret

        .data
msg:
        .string "Hello world\n"
