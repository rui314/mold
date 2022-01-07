#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

mkdir -p "$t"/sysroot/foo

cat <<EOF > "$t"/a.script
INPUT(=/foo/x.o)
EOF

cat <<EOF > "$t"/sysroot/b.script
INPUT(/foo/y.o)
EOF

cat <<EOF | $CC -c -o "$t"/sysroot/foo/x.o -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -c -o "$t"/sysroot/foo/y.o -xc -
#include <stdio.h>
void hello2() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -c -o "$t"/c.o -xc -
void hello();
void hello2();

int main() {
  hello();
  hello2();
}
EOF

$CC -B. -o "$t"/exe -Wl,--sysroot="$t"/sysroot \
  "$t"/a.script "$t"/sysroot/b.script "$t"/c.o

echo OK
