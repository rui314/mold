# RUN: llvm-mc -filetype=obj -triple=x86_64-unknown-linux %s -o %t.o
# RUN: mold -o %t.exe \
# RUN:   /usr/lib/x86_64-linux-gnu/crt1.o \
# RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
# RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbeginT.o \
# RUN:   %t.o \
# RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
# RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc_eh.a \
# RUN:   /usr/lib/x86_64-linux-gnu/libc.a \
# RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
# RUN:   /usr/lib/x86_64-linux-gnu/crtn.o

  .text
  .globl main
main:
  lea .Lhello(%rip), %rdi
  xor %eax, %eax
  call printf
  xor %edi, %edi
  call exit

  .data
.Lhello:
  .ascii "Hello world!\n"
