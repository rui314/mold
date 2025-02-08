#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
{
local:
  *;
global:
  xyz;
  foo*bar*[abc]x;
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
void xyz() {}
void foobarzx() {}
void foobarcx() {}
void foo123bar456bx() {}
void foo123bar456c() {}
void foo123bar456x() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o

readelf --dyn-syms $t/c.so > $t/log
grep -q ' xyz' $t/log
not grep -q ' foobarzx' $t/log
grep -q ' foobarcx' $t/log
grep -q ' foo123bar456bx' $t/log
not grep -q ' foo123bar456c' $t/log
not grep -q ' foo123bar456x' $t/log
