#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl get_sym
get_sym:
  la.global $a0, sym
  ld.w $a0, $a0, 0
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.globl sym
sym:
  .word 0xbeef
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

int get_sym();

int main() {
  printf("%x\n", get_sym());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--no-relax
$QEMU $t/exe1 | grep -Eq '^beef$'

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o
$QEMU $t/exe2 | grep -Eq '^beef$'

$OBJDUMP -d $t/exe2 | grep -A2 '<get_sym>:' | grep -Eq $'pcaddi'
