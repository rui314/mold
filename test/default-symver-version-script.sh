#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

echo '{ global: foo; local: *; };' > $t/b.ver

$CC -B. -o $t/c.so -shared $t/a.o -Wl,--default-symver -Wl,--version-script=$t/b.ver
readelf --dyn-syms $t/c.so | grep -F ' foo@@c.so'
