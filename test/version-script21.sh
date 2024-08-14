#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
VER1 { foo_x; };
VER2 { foo*; };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
void foo_x() {}
void foo_y() {}
void foo_z() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
grep -Fq 'foo_x@@VER1' $t/log
grep -Fq 'foo_y@@VER2' $t/log
grep -Fq 'foo_z@@VER2' $t/log
