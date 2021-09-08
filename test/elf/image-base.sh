#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -fuse-ld=$mold -no-pie -o $t/exe $t/a.o -Wl,--image-base=0x8000000
$t/exe | grep -q 'Hello world'
readelf -W --sections $t/exe | grep -Pq '.interp\s+PROGBITS\s+0000000008000...\b'

echo OK
