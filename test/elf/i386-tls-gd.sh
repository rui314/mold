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

cat <<EOF | cc -fPIC -c -o "$t"/a.o -xc - -m32
#include <stdio.h>

static _Thread_local int x1 = 1;
static _Thread_local int x2;
extern _Thread_local int x3;
extern _Thread_local int x4;
int get_x5();
int get_x6();

int main() {
  x2 = 2;

  printf("%d %d %d %d %d %d\n", x1, x2, x3, x4, get_x5(), get_x6());
  return 0;
}
EOF

cat <<EOF | cc -fPIC -c -o "$t"/b.o -xc - -m32
_Thread_local int x3 = 3;
static _Thread_local int x5 = 5;
int get_x5() { return x5; }
EOF


cat <<EOF | cc -fPIC -c -o "$t"/c.o -xc - -m32
_Thread_local int x4 = 4;
static _Thread_local int x6 = 6;
int get_x6() { return x6; }
EOF

clang -fuse-ld="$mold" -shared -o "$t"/d.so "$t"/b.o -m32
clang -fuse-ld="$mold" -shared -o "$t"/e.so "$t"/c.o -Wl,--no-relax -m32

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/d.so "$t"/e.so -m32
"$t"/exe | grep -q '1 2 3 4 5 6'

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/d.so "$t"/e.so -Wl,-no-relax -m32
"$t"/exe | grep -q '1 2 3 4 5 6'

echo OK
