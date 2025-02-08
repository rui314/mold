#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fPIE -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -pie -o $t/exe $t/a.o
readelf --file-header $t/exe | grep -E '(Shared object file|Position-Independent Executable file)'
$QEMU $t/exe | grep 'Hello world'
