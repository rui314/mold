#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

extern char section_distance[];

int main() {
  printf("distance: %ld\n", (long)section_distance);
  if ((long)section_distance == 0) {
    return 1;
  }
  return 0;
}
EOF

cat <<EOF > $t/script.ld
PROVIDE_HIDDEN(section_distance = ADDR(.data) - ADDR(.text));
EOF

# Link with -no-pie since the symbol is absolute and cannot be accessed PC-relatively in PIE
$CC -no-pie -B. -o $t/exe $t/a.o -T $t/script.ld
$QEMU $t/exe
