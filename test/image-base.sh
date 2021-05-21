#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,--image-base=0x8000000
$t/exe | grep -q 'Hello world'
readelf --sections $t/exe | grep -Pq '.interp\s+PROGBITS\s+00000000080002e0'

echo OK
