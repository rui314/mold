#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -c -o $t/a.o -xc -
int foo1() { return 1; }
int foo2() { return 2; }
int foo3() { return 3; }

__asm__(".symver foo1, foo@VER1");
__asm__(".symver foo2, foo@VER2");
__asm__(".symver foo3, foo@@VER3");
EOF

echo 'VER1 { local: *; }; VER2 { local: *; }; VER3 { local: *; };' > $t/b.ver
clang -fuse-ld=`pwd`/../mold -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/b.ver

cat <<EOF | cc -c -o $t/d.o -x assembler -
.globl bar1, bar2, bar3
bar1:
  call "foo@VER1"
  ret
bar2:
  call "foo@VER2"
  ret
EOF

cat <<EOF | cc -c -o $t/e.o -xc -
#include <stdio.h>

int bar1();
int bar2();

int main() {
  printf("%d %d\n", bar1(), bar2());
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/d.o $t/e.o $t/c.so
$t/exe | grep -q '^1 2$'

echo OK
