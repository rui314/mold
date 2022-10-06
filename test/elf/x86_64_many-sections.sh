#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

seq 1 65500 | sed 's/.*/.section .text.\0, "ax",@progbits/' > $t/a.s

$CC -c -o $t/a.o $t/a.s

cat <<'EOF' | $CC -c -xc -o $t/b.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q Hello
