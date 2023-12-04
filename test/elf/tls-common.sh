#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $GCC -o $t/a.o -c -xassembler -
.globl foo
.tls_common foo,4,4
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -std=c11 -
#include <stdio.h>

extern _Thread_local int foo;

int main() {
  printf("foo=%d\n", foo);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
readelf -WS $t/exe | grep -Fq .tls_common
$QEMU $t/exe | grep -q '^foo=0$'
