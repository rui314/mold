#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.so -fPIC -shared -xc -
void *foo() {
  return foo;
}

void *bar() {
  return bar;
}
EOF

cat <<EOF | cc -o $t/b.o -c -xc - -fPIC
void *bar();

void *baz() {
  return bar;
}
EOF

cat <<EOF | cc -o $t/c.o -c -xc - -fno-PIC
#include <stdio.h>

void *foo();
void *bar();
void *baz();

int main() {
  printf("%d %d %d\n", foo == foo(), bar == bar(), bar == baz());
}
EOF

clang -fuse-ld=$mold -no-pie -o $t/exe $t/a.so $t/b.o $t/c.o
$t/exe | grep -q '^1 1 1$'

echo OK
