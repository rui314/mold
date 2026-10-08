#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Tags 40 and 41 are not defined by the psABI. An even tag has a ULEB128
# value and an odd tag has a string value. The value 5 of tag 40 must not
# be mistaken for Tag_RISCV_arch.
cat <<EOF | $CC -o $t/a.o -c -x assembler -
.attribute stack_align, 16
.attribute 40, 5
.attribute 41, "foo"
.globl _start
_start:
  ret
EOF

$CC -B. -nostdlib -o $t/exe $t/a.o
readelf -A $t/exe > $t/log
grep -F 'Tag_RISCV_stack_align: 16-bytes' $t/log
grep -F 'Tag_RISCV_arch: "rv64i' $t/log
