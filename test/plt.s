// RUN: mold -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:   /home/ruiu/mold/test/Output/hello-dynamic.s.tmp.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
// RUN:   /lib/x86_64-linux-gnu/libc.so.6 \
// RUN:   /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
// RUN:   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2

// RUN: objdump -d -j .plt %t.exe
// CHECK: 2011a8: ff 35 b2 0e 00 00  pushq  0xeb2(%rip)  # 202060 <__init_array_end+0x30>
// CHECK: 2011ae: ff 25 b4 0e 00 00  jmpq   *0xeb4(%rip) # 202068 <_DYNAMIC>
// CHECK: 2011b4: 0f 1f 40 00        nopl   0x0(%rax)

        .globl main
main:
