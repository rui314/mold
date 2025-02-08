#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = ppc64 ] && skip

cat <<EOF | $CC -c -o $t/a.o -xc - -ffunction-sections 2> /dev/null
 __attribute__((retain)) int foo() {}
int bar() {}
int main() {}
EOF

# Older versions of GCC does not support __attribute__((retain))
readelf -WS $t/a.o | grep -q '\.text\.foo.*AXR' || skip

$CC -B. -o $t/exe $t/a.o -Wl,-gc-sections
nm $t/exe > $t/log
grep -q foo $t/log
not grep -q bar $t/log
