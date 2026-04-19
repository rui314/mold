#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,x86-64-v2
readelf -n $t/exe2 | grep -F 'Unknown note type: (0x00000005)' && skip
readelf -n $t/exe2 | grep -F 'procesor-specific type 0xc0008002' && skip
readelf -n $t/exe2 | grep 'x86 ISA needed: .*x86-64-v2'

$CC -B. -o $t/exe3 $t/a.o -Wl,-z,x86-64-v3
readelf -n $t/exe3 | grep 'x86 ISA needed: .*x86-64-v3'

$CC -B. -o $t/exe4 $t/a.o -Wl,-z,x86-64-v4
readelf -n $t/exe4 | grep 'x86 ISA needed: .*x86-64-v4'
