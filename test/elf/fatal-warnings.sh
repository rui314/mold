#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | clang -fcommon -xc -c -o "$t"/a.o -
int foo;
EOF

cat <<EOF | clang -fcommon -xc -c -o "$t"/b.o -
int foo;

int main() {
  return 0;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o \
  -Wl,-warn-common 2> /dev/null

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o \
  -Wl,-warn-common -Wl,-fatal-warnings 2> /dev/null || false

echo OK
