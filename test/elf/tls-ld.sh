#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

if [ "$(uname -m)" = x86_64 ]; then
  dialect=gnu
elif [ "$(uname -m)" = aarch64 ]; then
  dialect=trad
else
  echo skipped
  exit 0
fi

cat <<EOF | gcc -ftls-model=local-dynamic -mtls-dialect=$dialect -fPIC -c -o "$t"/a.o -xc -
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

cat <<EOF | gcc -ftls-model=local-dynamic -mtls-dialect=$dialect  -fPIC -c -o "$t"/b.o -xc -
_Thread_local int foo = 3;
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o
"$t"/exe | grep -q '3 5 3 5'

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -Wl,-no-relax
"$t"/exe | grep -q '3 5 3 5'

echo OK
