// RUN: cc -o %t.o -c %s

// RUN: mold -o %t.exe \
// RUN:  /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:  /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:  /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:  -L/usr/lib/gcc/x86_64-linux-gnu/9 \
// RUN:  -L/usr/lib/x86_64-linux-gnu \
// RUN:  -L/usr/lib64 \
// RUN:  -L/lib/x86_64-linux-gnu \
// RUN:  -L/lib64 \
// RUN:  -L/usr/lib/x86_64-linux-gnu \
// RUN:  -L/usr/lib64 \
// RUN:  -L/usr/lib64 \
// RUN:  -L/usr/lib \
// RUN:  -L/usr/lib/llvm-10/lib \
// RUN:  -L/lib \
// RUN:  -L/usr/lib \
// RUN:   %t.o \
// RUN:  -lgcc \
// RUN:  --as-needed \
// RUN:  -lgcc_s \
// RUN:  --no-as-needed \
// RUN:  -lc \
// RUN:  -lgcc \
// RUN:  --as-needed \
// RUN:  -lgcc_s \
// RUN:  --no-as-needed \
// RUN:  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:  /usr/lib/x86_64-linux-gnu/crtn.o

// RUN: %t.exe | grep 'Hello world'

        .text
        .globl main
main:
        lea msg(%rip), %rdi
        xor %rax, %rax
        call printf@PLT
        xor %rax, %rax
        ret

        .data
msg:
        .string "Hello world\n"
