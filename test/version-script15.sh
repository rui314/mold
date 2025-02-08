#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
{
local:
  *;
global:
  [abc][^abc][^\]a-zABC];
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
void azZ() {}
void czZ() {}
void azC() {}
void aaZ() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o

readelf --dyn-syms $t/c.so > $t/log
grep ' azZ' $t/log
grep ' czZ' $t/log
not grep ' azC' $t/log
not grep ' aaZ' $t/log
