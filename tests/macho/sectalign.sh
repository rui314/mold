#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section("__DATA,__blob"))) char blob[10] = "hi";
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__blob,0x4000
otool -l $t/exe > $t/log
# The section is 2^14-aligned.
grep -A6 'sectname __blob' $t/log | grep -q 'align 2\^14'
addr=$(grep -A2 'sectname __blob' $t/log | awk '/addr/{print $2}')
[ $(( addr % 0x4000 )) -eq 0 ]

# Alignment must be a power of two.
not $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__blob,0x3000 2> $t/log2
grep -q 'not a power of two' $t/log2
