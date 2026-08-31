#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
VER1 { foo*; };
VER2 { foo*bar*; };
VER3 { *she*; };
VER4 { *he*; };
VER5 { *aa*; };
VER6 { *bcaz*; };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
void foo_bar() {}
void she() {}
void bcaa() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
grep -F 'foo_bar@@VER2' $t/log
grep -F 'she@@VER4' $t/log
grep -F 'bcaa@@VER5' $t/log
