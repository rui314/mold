#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -ftls-model=local-dynamic -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

extern _Thread_local int foo;
static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

cat <<EOF | cc -ftls-model=local-dynamic -fPIC -c -o $t/b.o -xc -
_Thread_local int foo = 3;
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '3 5 3 5'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o -Wl,-no-relax
$t/exe | grep -q '3 5 3 5'

echo OK
