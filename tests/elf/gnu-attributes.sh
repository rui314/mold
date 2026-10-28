#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The output has one .gnu.attributes, merged as with GNU ld, not a copy
# per input.
cat <<EOF | $CC -c -o $t/a.o -xc -
double foo(double x) { return x * 2; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
double foo(double);
int main() { return foo(1); }
EOF

readelf -S $t/a.o | grep -F .gnu.attributes || skip

$CC -B. -o $t/exe $t/a.o $t/b.o
readelf -A $t/exe > $t/log 2>&1
grep -F Tag_GNU_ $t/log
not grep -Fi error $t/log

./mold -r -o $t/c.o $t/a.o $t/b.o
readelf -A $t/c.o > $t/log 2>&1
grep -F Tag_GNU_ $t/log
not grep -Fi error $t/log
