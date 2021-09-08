#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
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
#include <stdio.h>

extern _Thread_local int foo;
extern _Thread_local int bar;

int main() {
  bar = 7;
  printf("%d %d\n", foo, bar);
}
EOF

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -shared -o $t/b.so -xc -
_Thread_local int foo = 5;
_Thread_local int bar;
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.so
$t/exe | grep -q '5 7'

echo OK
