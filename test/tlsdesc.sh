#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | gcc -fPIC -mtls-dialect=gnu2 -c -o $t/a.o -xc -
extern _Thread_local int foo;

int get_foo() {
  return foo;
}
EOF

cat <<EOF | clang -fPIC -c -o $t/b.o -xc -
#include <stdio.h>

_Thread_local int foo;

int get_foo();

int main() {
  foo = 42;
  printf("%d\n", get_foo());
  return 0;
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 42

clang -fuse-ld=`pwd`/../mold -shared -o $t/c.so $t/a.o
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/b.o $t/c.so
$t/exe | grep -q 42

echo OK
