#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An executable that doesn't link against any DSO has no .dynamic, so it
# must not have .dynsym or .dynstr either, even if symbols are exported.
cat <<EOF | $CC -c -o $t/a.o -xc -
void foo() {}
void _start() {}
EOF

echo '{ foo; };' > $t/dyn

./mold -o $t/exe1 $t/a.o --dynamic-list=$t/dyn
readelf -SW $t/exe1 | not grep -E '\.dyn(sym|str)'

./mold -o $t/exe2 $t/a.o --export-dynamic
readelf -SW $t/exe2 | not grep -E '\.dyn(sym|str)'
