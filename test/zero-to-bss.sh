#!/bin/bash
. $(dirname $0)/common.inc

[ "$(uname)" = FreeBSD ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

__attribute__((section(".zero"))) char zero[256];

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf -WS $t/exe1 | grep -E '.zero\s+PROGBITS'
$QEMU $t/exe1 | grep Hello

$CC -B. -o $t/exe2 $t/a.o -Wl,--zero-to-bss
readelf -WS $t/exe2 | grep -E '.zero\s+NOBITS'
$QEMU $t/exe2 | grep Hello
