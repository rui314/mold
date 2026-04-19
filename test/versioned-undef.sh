#!/bin/bash
. $(dirname $0)/common.inc

# Skip if libc is musl because musl does not fully support GNU-style
# symbol versioning.
is_musl && skip

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo1() { return 1; }
int foo2() { return 2; }
int foo3() { return 3; }
int bar3() { return 4; }

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@VER2");
__asm__(".symver foo3, foo@@VER3");
__asm__(".symver bar3, bar@@VER3");
EOF

echo 'VER1 { local: *; }; VER2 { local: *; }; VER3 { local: *; };' > $t/b.ver
$CC -B. -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/b.ver

cat <<EOF | $CC -c -o $t/d.o -xc -
#include <stdio.h>

int foo1();
int foo2();
int foo3();
int foo();
int bar();

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@VER2");
__asm__(".symver foo3, foo@VER3");
__asm__(".symver bar, bar@VER3");

int main() {
  printf("%d %d %d %d %d\n", foo1(), foo2(), foo3(), foo(), bar());
}
EOF

$CC -B. -o $t/exe $t/d.o $t/c.so
$QEMU $t/exe | grep '^1 2 3 3 4$'
