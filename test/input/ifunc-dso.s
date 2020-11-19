        .text
real_foobar:
        lea     .Lmsg(%rip), %rdi
        xor     %rax, %rax
        call    printf
        xor     %rax, %rax
        ret

        .globl  resolve_foobar
resolve_foobar:
        pushq   %rbp
        movq    %rsp, %rbp
        movq    real_foobar@GOTPCREL(%rip), %rax
        popq    %rbp
        ret

        .globl  foobar
        .type   foobar, @gnu_indirect_function
        .set    foobar, resolve_foobar

        .data
.Lmsg:
        .string "Hello world\n"
