#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void foo();
void bar() { foo(); }
EOF

$CC -B. -shared -o $t/b.so $t/a.o
$CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,undefs

not $CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,defs |& \
  grep -q 'undefined symbol:.* foo'

not $CC -B. -shared -o $t/b.so $t/a.o -Wl,-no-undefined |& \
  grep -q 'undefined symbol:.* foo'

$CC -B. -shared -o $t/c.so $t/a.o -Wl,-z,defs -Wl,--warn-unresolved-symbols |& \
  grep -q 'undefined symbol:.* foo$'
