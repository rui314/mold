#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fuse-ld=gold -shared -fPIC -o $t/a.so -xc -
int foo() __attribute__((visibility("protected")));
int bar() __attribute__((visibility("protected")));
void *baz() __attribute__((visibility("protected")));

int foo() {
  return 4;
}

int bar() {
  return foo();
}

void *baz() {
  return baz;
}
EOF

cat <<EOF | cc -c -o $t/b.o -xc - -fno-PIE
#include <stdio.h>

int foo() {
  return 3;
}

int bar();
void *baz();

int main() {
  printf("%d %d %d\n", foo(), bar(), baz == baz());
}
EOF

clang -fuse-ld=`pwd`/../mold -no-pie -o $t/exe $t/b.o $t/a.so
$t/exe | grep -q '3 4 0'

echo OK
