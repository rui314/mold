#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | cc -c -fPIC -o"$t"/a.o -xc -
int foo = 4;

int get_foo() {
  return foo;
}

void *bar() {
  return bar;
}
EOF

clang -fuse-ld="$mold" -shared -fPIC -o "$t"/b.so "$t"/a.o -Wl,-Bsymbolic

cat <<EOF | cc -c -o "$t"/c.o -xc - -fno-PIE
#include <stdio.h>

extern int foo;
int get_foo();
void *bar();

int main() {
  foo = 3;
  printf("%d %d %d\n", foo, get_foo(), bar == bar());
}
EOF

clang -fuse-ld="$mold" -no-pie -o "$t"/exe "$t"/c.o "$t"/b.so
"$t"/exe | grep -q '3 4 0'

echo OK
