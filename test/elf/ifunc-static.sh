#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

# Skip if libc is musl because musl does not support GNU FUNC
echo 'int main() {}' | cc -o "$t"/exe -xc -
ldd "$t"/exe | grep -q ld-musl && { echo OK; exit; }

cat <<EOF | clang -o "$t"/a.o -c -xc -
#include <stdio.h>

void foo() __attribute__((ifunc("resolve_foo")));

void hello() {
  printf("Hello world\n");
}

void *resolve_foo() {
  return hello;
}

int main() {
  foo();
  return 0;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -static
"$t"/exe | grep -q 'Hello world'

echo OK
