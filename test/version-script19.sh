#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
{ local: extern "C++" { foo*; }; };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc++ -
void foobar() {}
void baz() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
not grep -Eq foobar $t/log
grep -Eq 'GLOBAL.*baz' $t/log
