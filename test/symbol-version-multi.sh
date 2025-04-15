#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
void foo() {}
__asm__(".symver foo, foo@TEST1");
__asm__(".symver foo, foo@@TEST2");
EOF

cat <<EOF > $t/b.version
TEST1 { local: *; };
TEST2 {};
TEST3 {};
EOF

$CC -B. -o $t/c.so -shared $t/a.o -Wl,--version-script=$t/b.version

readelf -W --dyn-syms $t/c.so | grep -F 'foo@TEST1'
readelf -W --dyn-syms $t/c.so | grep -F 'foo@@TEST2'
