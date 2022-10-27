#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC -
__attribute__((section(".foo"))) int foo;
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o -no-pie -Wl,--section-align=.foo=0x2000
readelf -WS $t/exe1 | grep -q '\.foo.* 8192$'

$CC -B. -o $t/exe1 $t/a.o -no-pie -Wl,--section-align=.foo=256
readelf -WS $t/exe1 | grep -q '\.foo.* 256$'
