#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
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
EOF

../mold -o $t/exe /usr/lib/x86_64-linux-gnu/crt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
  $t/a.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
  /usr/lib/x86_64-linux-gnu/crtn.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
  /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
  /lib/x86_64-linux-gnu/libc.so.6 \
  /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
  /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2

readelf --sections $t/exe | grep -q "
  [17] .got              PROGBITS         0000000000202000  00002000
       0000000000000028  0000000000000000  WA       0     0     8
  [18] .got.plt          PROGBITS         0000000000202028  00002028
       0000000000000020  0000000000000000  WA       0     0     8
"

readelf -x .got.plt $t/exe | grep -q "
Hex dump of section '.got.plt':
  0x00202028 48202000 00000000 00000000 00000000 H  .............
  0x00202038 00000000 00000000 16102000 00000000 .......... .....
"

objdump -d -j .plt $t/exe | grep -q "
Disassembly of section .plt:

0000000000201000 <printf@plt-0x10>:
  201000: ff 35 2a 10 00 00     pushq  0x102a(%rip)        # 202030 <_GLOBAL_OFFSET_TABLE_+0x8>
  201006: ff 25 2c 10 00 00     jmpq   *0x102c(%rip)        # 202038 <_GLOBAL_OFFSET_TABLE_+0x10>
  20100c: 0f 1f 40 00           nopl   0x0(%rax)

0000000000201010 <printf@plt>:
  201010: ff 25 2a 10 00 00     jmpq   *0x102a(%rip)        # 202040 <printf@GLIBC_2.2.5>
  201016: 68 00 00 00 00        pushq  $0x0
  20101b: e9 e0 ff ff ff        jmpq   201000 <_IO_stdin_used+0xb80>
"

echo OK
