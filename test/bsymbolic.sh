#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -fPIC -o $t/a.so -xc - -Wl,-Bsymbolic
void *foo() {
  return foo;
}
EOF

cat <<EOF | cc -fuse-ld=gold -shared -fPIC -o $t/b.so -xc -
void *bar() __attribute__((visibility("protected")));

void *bar() {
  return bar;
}
EOF

cat <<EOF | cc -c -o $t/c.o -xc - -fno-PIE
#include <stdio.h>

void *foo();
void *bar();

int main() {
  printf("%p %p %p %p\n", foo, foo(), bar, bar());
}
EOF

cc -fuse-ld=gold -no-pie -o $t/exe $t/c.o $t/a.so $t/b.so
$t/exe

echo OK
