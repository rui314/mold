#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void _start() {}
EOF

$STRIP --remove-section=.riscv.attributes $t/a.o

$CC -B. -nostdlib -o $t/exe $t/a.o

readelf -W --segments --sections $t/exe > $t/log
! grep -F .riscv.attributes $t/log || false
! grep -F RISCV_ATTR $t/log || false
