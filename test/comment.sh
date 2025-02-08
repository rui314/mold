#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o
readelf -p .comment $t/exe | grep mold
readelf -SW $t/exe | grep -E '\.comment.*\bMS\b'
