#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

cat <<EOF | $CC -c -o $t/a.o -xc - -fno-PIC
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. $t/a.o -o $t/exe -static -Wl,--omagic
readelf -W --segments $t/exe | grep -qw RWE
