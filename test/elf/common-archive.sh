#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fcommon -xc -c -o $t/a.o -
#include <stdio.h>

int foo;
int bar;
__attribute__((weak)) int two();

int main() {
  printf("%d %d %d\n", foo, bar, two ? two() : -1);
}
EOF

cat <<EOF | cc -fcommon -xc -c -o $t/b.o -
int foo = 5;
EOF

cat <<EOF | cc -fcommon -xc -c -o $t/c.o -
int bar;
int two() { return 2; }
EOF

rm -f $t/d.a
ar rcs $t/d.a $t/b.o $t/c.o

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/d.a
$t/exe | grep -q '5 0 -1'

cat <<EOF | cc -fcommon -xc -c -o $t/e.o -
int bar = 0;
int two() { return 2; }
EOF

rm -f $t/e.a
ar rcs $t/e.a $t/b.o $t/e.o

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/e.a
$t/exe | grep -q '5 0 2'

echo OK
