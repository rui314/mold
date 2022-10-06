#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -c -fPIC -o $t/a.o -xc -
void foo() {}
void bar() {}
EOF

cat <<EOF > $t/b.script
{
  global: foo;
  local: *;
};
EOF

$CC -B. -shared -o $t/c.so -flto $t/a.o -Wl,-version-script=$t/b.script
nm -D $t/c.so | grep -q 'T foo'
! nm -D $t/c.so | grep -q 'T bar' || false
