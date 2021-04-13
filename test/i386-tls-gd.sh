#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -c -o $t/a.o -xc - -m32
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

cat <<EOF | cc -fPIC -shared -o $t/b.so -xc - -m32
_Thread_local int foo = 3;
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.so -m32
$t/exe | grep -q '3 5 3 5'

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.so -Wl,-no-relax -m32
$t/exe | grep -q '3 5 3 5'

echo OK
