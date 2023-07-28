#!/bin/bash
. $(dirname $0)/common.inc

seq 1 100000 | sed 's/.*/.section .data.\0,"aw"\n.word 0\n/g' | \
  $CC -c -xassembler -o $t/a.o -

cat <<'EOF' | $CC -c -xc -o $t/b.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q Hello
