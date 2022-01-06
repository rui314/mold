#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -fno-PIC -o "$t"/a.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;

int main() {
  printf("%d %d %d\n", foo, bar, &foo == &bar);
  return 0;
}
EOF

cat <<EOF | cc -fPIC -o "$t"/b.o -c -xc -
int foo = 42;
extern int bar __attribute__((alias("foo")));
extern int baz __attribute__((alias("foo")));
EOF

clang -fuse-ld="$mold" -shared -o "$t"/c.so "$t"/b.o
clang -fuse-ld="$mold" -no-pie -o "$t"/exe "$t"/a.o "$t"/c.so
"$t"/exe | grep -q '42 42 1'

echo OK
