#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
void foo1() { printf("foo1\n"); }
void foo2() { printf("foo2\n"); }
__asm__(".symver foo1, foo@ver1");
__asm__(".symver foo2, foo@@ver2");
EOF

echo 'ver1 { local: *; }; ver2 { local: *; };' > $t/b.ver
$CC -B. -shared -o $t/libfoo.so $t/a.o -Wl,--version-script=$t/b.ver

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo();
int main() { foo(); }
__asm__(".symver foo, foo@ver2");
EOF

$CC -B. -o $t/exe $t/b.o -L$t -Wl,--as-needed -lfoo -Wl,-rpath=$t
$QEMU $t/exe | grep foo2
