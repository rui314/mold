#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fcommon -xc -c -o $t/a.o -
#include <stdio.h>

int bar;

int main() {
  printf("%d\n", bar);
}
EOF

cat <<EOF | cc -fcommon -xc -c -o $t/b.o -
int foo;
EOF

rm -f $t/c.a
ar rcs $t/c.a $t/b.o

cat <<EOF | cc -fcommon -xc -c -o $t/d.o -
int foo;
int bar = 5;
int get_foo() { return foo; }
EOF

rm -f $t/e.a
ar rcs $t/e.a $t/d.o

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/c.a $t/e.a
$t/exe | grep -q 5

echo OK
