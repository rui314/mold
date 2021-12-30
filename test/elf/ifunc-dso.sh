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

cat <<EOF | cc -fPIC -o "$t"/a.o -c -xc -
void foobar(void);

int main() {
  foobar();
}
EOF

cat <<EOF | cc -fPIC -shared -o "$t"/b.so -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
void foobar(void);

static void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

static Func *resolve_foobar(void) {
  return real_foobar;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.so
"$t"/exe | grep -q 'Hello world'

echo OK
