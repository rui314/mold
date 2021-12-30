#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | cc -shared -o "$t"/a.dylib -xc -
_Thread_local int a;
EOF

cat <<EOF | cc -o "$t"/b.o -c -xc -
#include <stdio.h>

extern _Thread_local int a;

int main() {
  printf("%d\n", a);
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.dylib "$t"/b.o
"$t"/exe | grep -q '^0$'

echo OK
