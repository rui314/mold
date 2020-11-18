// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:   %t.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
// RUN:   /lib/x86_64-linux-gnu/libc.so.6 \
// RUN:   /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
// RUN:   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2 \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o
// RUN: %t.exe | grep 'Hello world'

        .text
        .globl  real_foobar
real_foobar:
        lea     msg(%rip), %rdi
        xor     %rax, %rax
        call    printf
        xor     %rax, %rax
        ret

        .globl  resolve_foobar
resolve_foobar:
        pushq   %rbp
        movq    %rsp, %rbp
        leaq    real_foobar(%rip), %rax
        popq    %rbp
        ret

        .globl  foobar
        .type   foobar, @gnu_indirect_function
        .set    foobar, resolve_foobar

        .globl  main
main:
        pushq   %rbp
        movq    %rsp, %rbp
        call    foobar@PLT
	xor     %rax, %rax
        popq    %rbp
        ret

        .data
msg:
        .string "Hello world\n"
