#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -no-pie
readelf -W --sections $t/exe | grep -Fw .note.gnu.property || skip
readelf -W --segments $t/exe | grep -qw GNU_PROPERTY
