#!/usr/bin/env bash
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
grep -E ' azZ( |$)' $t/log
grep -E ' czZ( |$)' $t/log
not grep -E ' azC( |$)' $t/log
not grep -E ' aaZ( |$)' $t/log
grep -E ' azZ_( |$)' $t/log
grep -E ' czZ_( |$)' $t/log
not grep -E ' azC_( |$)' $t/log
not grep -E ' aaZ_( |$)' $t/log
