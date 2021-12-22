#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

if [ "$(uname -m)" = x86_64 ]; then
  dialect=gnu2
elif [ "$(uname -m)" = aarch64 ]; then
  dialect=desc
else
  echo skipped
  exit 0
fi

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -c -o "$t"/a.o -xc -
#include <stdio.h>

extern _Thread_local int foo;

int main() {
  foo = 42;
  printf("%d\n", foo);
}
EOF

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -c -o "$t"/b.o -xc -
_Thread_local int foo;
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -static
"$t"/exe | grep -q 42

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -static -Wl,-no-relax
"$t"/exe | grep -q 42

echo OK
