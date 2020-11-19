// RUN: cc -shared -fPIC -o %t.so %p/input/ifunc-dso.s
// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:   %t.o \
// RUN:   %t.so \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
// RUN:   /lib/x86_64-linux-gnu/libc.so.6 \
// RUN:   /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
// RUN:   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2 \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o
// RUN: %t.exe | grep 'Hello world'

        .text
        .globl  main
main:
        pushq   %rbp
        movq    %rsp, %rbp
        call    foobar@PLT
	xor     %rax, %rax
        popq    %rbp
        ret
