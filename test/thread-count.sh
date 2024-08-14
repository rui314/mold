#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-no-threads
$CC -B. -o $t/exe $t/a.o -Wl,-thread-count=1
$CC -B. -o $t/exe $t/a.o -Wl,-threads
$CC -B. -o $t/exe $t/a.o -Wl,-threads=1
$CC -B. -o $t/exe $t/a.o -Wl,--threads=1
