#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
VER1 { foo*; };
VER2 { foo*bar*; };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
void foo_bar() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
grep -F 'foo_bar@@VER2' $t/log
