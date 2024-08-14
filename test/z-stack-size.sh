#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,stack-size=0x900000
readelf -W --segments $t/exe | grep -q 'GNU_STACK .* 0x900000 RW'
