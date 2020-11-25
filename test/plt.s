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
// SECTIONS:   [15] .got              PROGBITS         0000000000202000  00002000
// SECTIONS:        0000000000000028  0000000000000000  WA       0     0     8
// SECTIONS:   [16] .got.plt          PROGBITS         0000000000202028  00002028
// SECTIONS:        0000000000000020  0000000000000000  WA       0     0     8

// RUN: readelf -x .got.plt %t.exe | FileCheck --check-prefix=GOTPLT %s
// GOTPLT: Hex dump of section '.got.plt':
// GOTPLT:   0x00202028 48202000 00000000 00000000 00000000 H  .............
// GOTPLT:   0x00202038 00000000 00000000 16102000 00000000 .......... .....

// RUN: objdump -d -j .plt %t.exe | FileCheck --check-prefix=PLT %s
// PLT: Disassembly of section .plt:
// PLT: 0000000000201000 <printf@plt-0x10>:
// PLT:   201000:       ff 35 2a 10 00 00       pushq  0x102a(%rip)        # 202030 <_GLOBAL_OFFSET_TABLE_+0x8>
// PLT:   201006:       ff 25 2c 10 00 00       jmpq   *0x102c(%rip)        # 202038 <_GLOBAL_OFFSET_TABLE_+0x10>
// PLT:   20100c:       0f 1f 40 00             nopl   0x0(%rax)
// PLT: 0000000000201010 <printf@plt>:
// PLT:   201010:       ff 25 2a 10 00 00       jmpq   *0x102a(%rip)        # 202040 <printf>
// PLT:   201016:       68 00 00 00 00          pushq  $0x0
// PLT:   20101b:       e9 e0 ff ff ff          jmpq   201000 <_IO_stdin_used+0xac8>

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
