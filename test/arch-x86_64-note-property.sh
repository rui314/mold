#!/bin/bash
. $(dirname $0)/common.inc

echo endbr64 | $CC -o /dev/null -c -xassembler - 2> /dev/null || skip
test_cflags -fcf-protection=branch || skip

cat <<EOF | $CC -fcf-protection=branch -c -o $t/a.o -xc -
void _start() {}
EOF

cat <<EOF | $CC -fcf-protection=none -c -o $t/b.o -xc -
void _start() {}
EOF

./mold -o $t/exe $t/a.o
readelf -n $t/exe | grep 'x86 feature: IBT'

./mold -o $t/exe $t/b.o
readelf -n $t/exe | not grep 'x86 feature: IBT'
