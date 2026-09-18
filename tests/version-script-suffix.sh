#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.ver
ver1 {
  global: *_v1;
  local: *;
};

ver2 {
  global: foo_*;
};

ver3 {
  global: foo_v1;
};
EOF

cat <<EOF | $CC -B. -xc -shared -o $t/b.so -Wl,-version-script,$t/a.ver -
void bar_v1() {}
void foo_bar() {}
void foo_v1() {}
void baz() {}
EOF

readelf --dyn-syms $t/b.so > $t/log
grep -F 'bar_v1@@ver1' $t/log
grep -F 'foo_bar@@ver2' $t/log
grep -F 'foo_v1@@ver3' $t/log
! grep -wF baz $t/log || false
