#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

if [ $(uname -m) = x86_64 ]; then
  dialect=gnu2
elif [ $(uname -m) = aarch64 ]; then
  dialect=desc
else
  echo skipped
  exit 0
fi

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -c -o $t/a.o -xc -
extern _Thread_local int foo;

int get_foo() {
  return foo;
}

static _Thread_local int bar = 5;

int get_bar() {
  return bar;
}
EOF

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -c -o $t/b.o -xc -
#include <stdio.h>

_Thread_local int foo;

int get_foo();
int get_bar();

int main() {
  foo = 42;
  printf("%d %d\n", get_foo(), get_bar());
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '42 5'

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o -Wl,-no-relax
$t/exe | grep -q '42 5'

clang -fuse-ld=$mold -shared -o $t/c.so $t/a.o
clang -fuse-ld=$mold -o $t/exe $t/b.o $t/c.so
$t/exe | grep -q '42 5'

clang -fuse-ld=$mold -shared -o $t/c.so $t/a.o -Wl,-no-relax
clang -fuse-ld=$mold -o $t/exe $t/b.o $t/c.so -Wl,-no-relax
$t/exe | grep -q '42 5'

echo OK
