#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

if [ $(uname -m) = x86_64 ]; then
  dialect=gnu
elif [ $(uname -m) = aarch64 ]; then
  dialect=trad
else
  echo skipped
  exit 0
fi

cat <<EOF | gcc -ftls-model=initial-exec -mtls-dialect=$dialect -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

static _Thread_local int foo;
static _Thread_local int bar;

void set() {
  foo = 3;
  bar = 5;
}

void print() {
  printf("%d %d ", foo, bar);
}
EOF

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o

cat <<EOF | gcc -c -o $t/c.o -xc -
#include <stdio.h>

_Thread_local int baz;

void set();
void print();

int main() {
  baz = 7;
  print();
  set();
  print();
  printf("%d\n", baz);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/b.so $t/c.o
$t/exe | grep -q '^0 0 3 5 7$'

clang -fuse-ld=$mold -o $t/exe $t/b.so $t/c.o -Wl,-no-relax
$t/exe | grep -q '^0 0 3 5 7$'

echo OK
