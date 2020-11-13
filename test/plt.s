// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:   %t.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
// RUN:   /lib/x86_64-linux-gnu/libc.so.6 \
// RUN:   /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
// RUN:   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2

// RUN: readelf --sections %t.exe | FileCheck --check-prefix=SECTIONS %s
// SECTIONS: [19] .got.plt          PROGBITS         0000000000202058  00002058
// SECTIONS:      0000000000000028  0000000000000000  WA       0     0     8
// SECTIONS: [20] .dynamic          DYNAMIC          0000000000202080  00002080
// SECTIONS:      0000000000000130  0000000000000010  WA       9     0     8

// RUN: readelf -x .got.plt %t.exe | FileCheck --check-prefix=GOTPLT %s
// GOTPLT: 0x00202058 80202000 00000000 00000000 00000000

// RUN: objdump -d -j .plt %t.exe | FileCheck --check-prefix=PLT %s
// PLT: 2011a8: ff 35 b2 0e 00 00       pushq  0xeb2(%rip)   # 202060
// PLT: 2011ae: ff 25 b4 0e 00 00       jmpq   *0xeb4(%rip)  # 202068
// PLT: 2011b4: 0f 1f 40 00             nopl   0x0(%rax)

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
