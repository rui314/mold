#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -xc -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld=$mold -Wl,--verbose -o $t/exe $t/a.o > /dev/null

echo OK
