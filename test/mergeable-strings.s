// RUN: cc -o %t.o -c %s
// RUN: mold -static -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbeginT.o \
// RUN:   %t.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc_eh.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libc.a \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o
// RUN: %t.exe | grep 'Hello world'

        .text
        .globl main
main:
        mov $.rodata.str1.1, %rdi
        xor %rax, %rax
        call printf
        mov $.rodata.str1.1+9, %rdi
        xor %rax, %rax
        call printf
        xor %rax, %rax
        ret

        .section .rodata.str1.1, "aMS", @progbits, 1
.L.str:
        .string "Hello"
        .string "foo world\n"
