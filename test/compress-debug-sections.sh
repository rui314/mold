#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib
readelf -tW $t/exe > $t/log
grep -F -A2 '.debug_info' $t/log | tr '\n' ' ' | grep -qw 'COMPRESSED'
grep -F -A2 '.debug_str' $t/log | tr '\n' ' ' | grep -qw 'COMPRESSED'
