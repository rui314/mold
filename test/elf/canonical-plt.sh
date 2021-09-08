#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.so -fPIC -shared -xc -
void *foo() {
  return foo;
}
EOF

cat <<EOF | cc -o $t/b.o -c -xc - -fno-PIC
#include <stdio.h>

void *foo();

int main() {
  printf("%d\n", foo == foo());
}
EOF

clang -fuse-ld=$mold -no-pie -o $t/exe $t/a.so $t/b.o
$t/exe | grep -q 1

echo OK
