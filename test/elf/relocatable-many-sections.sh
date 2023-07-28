#!/bin/bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep -q '__tsan_init' && skip

seq 1 80000 | sed 's/.*/.section .data.\0,"aw"\n.word 0\n/g' | \
  $CC -c -xassembler -o $t/a.o -

cat <<'EOF' | $CC -c -xc -o $t/b.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o
$CC -B. -o $t/exe $t/c.o
$QEMU $t/exe | grep -q Hello
