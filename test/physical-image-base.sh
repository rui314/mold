#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = ppc64 ] && skip

# Test if grep supports backreferences
echo abab | grep -E '(ab)\1' || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

__attribute__((section("foo"))) int bar;

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -no-pie -o $t/exe1 $t/a.o -Wl,--image-base=0x200000 \
   -Wl,--physical-image-base=0x800000

$QEMU $t/exe1 | grep 'Hello world'

readelf -W --segments $t/exe1 | grep -E 'LOAD\s+0x000000 0x0*200000 0x0*800000'
readelf -Ws $t/exe1 | grep __phys_start_foo


$CC -B. -no-pie -o $t/exe2 $t/a.o -Wl,--physical-image-base=0x800000 \
  -Wl,--section-order='=0x800000 TEXT RODATA =0x900000 DATA BSS'

readelf -W --segments $t/exe2 | grep -E 'LOAD\s+\S+\s+(\S+)\s\1.*R E 0'
readelf -W --segments $t/exe2 | grep -E 'LOAD\s+\S+\s+(\S+)\s\1.*R   0'
