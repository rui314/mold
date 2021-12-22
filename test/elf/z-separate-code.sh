#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe1 "$t"/a.o -Wl,-z,separate-loadable-segments
"$t"/exe1 | grep -q 'Hello world'

clang -fuse-ld="$mold" -o "$t"/exe2 "$t"/a.o -Wl,-z,separate-code -Wl,-z,norelro
"$t"/exe2 | grep -q 'Hello world'

clang -fuse-ld="$mold" -o "$t"/exe3 "$t"/a.o -Wl,-z,noseparate-code -Wl,-z,norelro
"$t"/exe3 | grep -q 'Hello world'

readelf --segments "$t"/exe3 > "$t"/log
! grep 'LOAD .* RW ' "$t"/log || false

echo OK
