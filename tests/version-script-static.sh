#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
void _start() {}
EOF

cat <<EOF > $t/b.ver
V1 { local: *; };
EOF

./mold -o $t/exe1 $t/a.o --version-script=$t/b.ver
readelf -SW $t/exe1 | not grep -F .gnu.version

./mold -o $t/exe2 $t/a.o --default-symver
readelf -SW $t/exe2 | not grep -F .gnu.version
