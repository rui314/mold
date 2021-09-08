#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
void foo() {}
EOF

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o
! readelf --dynamic $t/b.so | grep -Pq 'Flags: NODUMP' || false

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o -Wl,-z,nodump
readelf --dynamic $t/b.so | grep -Pq 'Flags: NODUMP'

echo OK
