#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf --dynamic $t/exe1 | not grep 'Audit library'

$CC -B. -o $t/exe2 $t/a.o -Wl,--audit=foo
readelf --dynamic $t/exe2 | grep -F 'Audit library: [foo]'

$CC -B. -o $t/exe3 $t/a.o -Wl,--audit=foo -Wl,--audit=bar
readelf --dynamic $t/exe3 | grep -F 'Audit library: [foo:bar]'
