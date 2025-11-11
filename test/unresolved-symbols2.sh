#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -fPIC -
int foo();
int bar() { foo(); }
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,defs -Wl,--unresolved-symbols,ignore-in-object-files
readelf -W --dyn-syms $t/b.so | grep -E ' UND foo(@@)?$'
