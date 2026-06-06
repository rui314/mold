#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = aarch64 ] || skip
command -v timeout >/dev/null || skip

cat <<EOF | $CC -c -o $t/a.o -fPIC -xassembler - 2> /dev/null || skip
.section .custom_rodata_a, "a", %progbits
.hidden A
.global A
A:
  .space 8

.section .custom_rodata_b_huge, "a", %progbits
.balign 16384
.hidden B
.global B
B:
  .space 8

.section .data, "aw", %progbits
.rept 7439
  .quad A
.endr

.global P_1
P_1:
  .quad A

.global P_2
P_2:
  .quad B
EOF

timeout 5s ./mold -m aarch64elf -shared -o $t/b.so $t/a.o \
  --pack-dyn-relocs=android -z max-page-size=16384
