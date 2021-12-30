#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang++ -c -o "$t"/a.o -xc++ -
int one() { return 1; }

struct Foo {
  int three() { static int x = 3; return x++; }
};

int a() {
  Foo x;
  return x.three();
}
EOF

cat <<EOF | clang++ -c -o "$t"/b.o -xc++ -
int two() { return 2; }

struct Foo {
  int three() { static int x = 3; return x++; }
};

int b() {
  Foo x;
  return x.three();
}
EOF

"$mold" --relocatable -o "$t"/c.o "$t"/a.o "$t"/b.o

[ -f "$t"/c.o ]
! [ -x t/c.o ] || false

cat <<EOF | clang++ -c -o "$t"/d.o -xc++ -
#include <iostream>

int one();
int two();

struct Foo {
  int three();
};

int main() {
  Foo x;
  std::cout << one() << " " << two() << " " << x.three() << "\n";
}
EOF

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/c.o "$t"/d.o
"$t"/exe | grep -q '^1 2 3$'

echo OK
