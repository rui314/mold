#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int two() { return 2; }
EOF

head -c 1 /dev/zero >> $t/a.o

cat <<EOF | cc -o $t/b.o -c -xc -
int three() { return 3; }
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

int two();
int three();

int main() {
  printf("%d\n", two() + three());
}
EOF

rm -f $t/d.a
ar rcs $t/d.a $t/a.o $t/b.o

clang -fuse-ld=$mold -o $t/exe $t/c.o $t/d.a

echo OK
