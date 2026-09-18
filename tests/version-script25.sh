#!/usr/bin/env bash
. $(dirname $0)/common.inc

# `local:` and `global:` do not have to be followed by a space.

cat <<'EOF' > $t/a.ver
VER1 {
  global:foo;
  local:*;
};
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
void foo() {}
void bar() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o

readelf --dyn-syms $t/c.so > $t/log
grep -F 'foo@@VER1' $t/log
not grep -F 'bar' $t/log
