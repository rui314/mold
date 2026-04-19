#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xc -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -Wl,--verbose -o $t/exe $t/a.o > /dev/null
