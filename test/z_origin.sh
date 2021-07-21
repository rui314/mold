#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-z,origin

readelf --dynamic $t/exe | grep -Pq '\(FLAGS\)\s+ORIGIN'
readelf --dynamic $t/exe | grep -Pq 'Flags: ORIGIN'

echo OK
