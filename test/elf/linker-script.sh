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

cat <<EOF > "$t"/script
GROUP("$t/a.o")
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/script
"$t"/exe | grep -q 'Hello world'

clang -fuse-ld="$mold" -o "$t"/exe -Wl,-T,"$t"/script
"$t"/exe | grep -q 'Hello world'

clang -fuse-ld="$mold" -o "$t"/exe -Wl,--script,"$t"/script
"$t"/exe | grep -q 'Hello world'

echo OK
