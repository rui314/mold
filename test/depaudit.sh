#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf --dynamic $t/exe1 | not grep 'Dependency audit library'

$CC -B. -o $t/exe2 $t/a.o -Wl,--depaudit=libdepaudit.so
readelf --dynamic $t/exe2 | grep -F 'Dependency audit library: [libdepaudit.so]'

$CC -B. -o $t/exe3 $t/a.o -Wl,--depaudit=foo -Wl,-P,bar
readelf --dynamic $t/exe3 | grep -F 'Dependency audit library: [foo:bar]'
