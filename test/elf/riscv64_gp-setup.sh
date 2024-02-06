#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] || skip

# Modified from arch/riscv64/crt_arch.h from musl.
cat <<EOF | $CC -o $t/dlstart.o -c -xassembler -
.section .sdata,"aw"

.text
.global _dlstart
.type _dlstart,%function
_dlstart:
.weak __global_pointer$
.hidden __global_pointer$
.option push
.option norelax
  lla gp, __global_pointer$
.option pop
  mv a0, sp
.weak _DYNAMIC
.hidden _DYNAMIC
  lla a1, _DYNAMIC
  andi sp, sp, -16
  tail _dlstart_c

.type _dlstart_c,%function
_dlstart_c:
  # The actual code here does not matter.
  ret
EOF

./mold -o $t/libc.so $t/dlstart.o -shared -e _dlstart

$OBJDUMP -d $t/libc.so | grep -q -E 'auipc\s+gp,'
$OBJDUMP -d $t/libc.so | grep -q -E 'addi\s+gp,\s*gp,'
