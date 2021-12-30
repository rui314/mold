#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | cc -c -o "$t"/a.o -xc -
#include <stdio.h>
char world[] = "world";

char *hello() {
  return "Hello";
}
EOF

clang -fuse-ld="$mold" -o "$t"/b.dylib -shared "$t"/a.o

cat <<EOF | cc -o "$t"/c.o -c -xc -
#include <stdio.h>

char *hello();
extern char world[];

int main() {
  printf("%s %s\n", hello(), world);
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/c.o "$t"/b.dylib
"$t"/exe | grep -q 'Hello world'

echo OK
