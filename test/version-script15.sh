#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
{
local:
  *;
global:
  [abc][^abc][^\]a-zABC];
  [abc][!abc][!\]a-zABC]_;
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
void azZ() {}
void czZ() {}
void azC() {}
void aaZ() {}
void azZ_() {}
void czZ_() {}
void azC_() {}
void aaZ_() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o

readelf --dyn-syms $t/c.so > $t/log
grep ' azZ$' $t/log
grep ' czZ$' $t/log
not grep ' azC$' $t/log
not grep ' aaZ$' $t/log
grep ' azZ_$' $t/log
grep ' czZ_$' $t/log
not grep ' azC_$' $t/log
not grep ' aaZ_$' $t/log
