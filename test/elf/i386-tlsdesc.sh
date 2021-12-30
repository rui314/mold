#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

echo 'int main() {}' | cc -m32 -o "$t"/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | gcc -fPIC -mtls-dialect=gnu2 -c -o "$t"/a.o -xc - -m32
extern _Thread_local int foo;

int get_foo() {
  return foo;
}
EOF

cat <<EOF | clang -fPIC -c -o "$t"/b.o -xc - -m32
#include <stdio.h>

_Thread_local int foo;

int get_foo();

int main() {
  foo = 42;
  printf("%d\n", get_foo());
  return 0;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -m32
"$t"/exe | grep -q 42

clang -fuse-ld="$mold" -shared -o "$t"/c.so "$t"/a.o -m32
clang -fuse-ld="$mold" -o "$t"/exe "$t"/b.o "$t"/c.so -m32
"$t"/exe | grep -q 42

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -Wl,-no-relax -m32
"$t"/exe | grep -q 42

clang -fuse-ld="$mold" -shared -o "$t"/c.so "$t"/a.o -Wl,-no-relax -m32
clang -fuse-ld="$mold" -o "$t"/exe "$t"/b.o "$t"/c.so -Wl,-no-relax -m32
"$t"/exe | grep -q 42

echo OK
