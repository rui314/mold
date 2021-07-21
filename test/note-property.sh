#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fcf-protection=branch -c -o $t/a.o -xc -
void _start() {}
EOF

cat <<EOF | clang -fcf-protection=none -c -o $t/b.o -xc -
void _start() {}
EOF

$mold -o $t/exe $t/a.o
readelf -n $t/exe | grep -q 'x86 feature: IBT'

$mold -o $t/exe $t/b.o
! readelf -n $t/exe | grep -q 'x86 feature: IBT' || false

echo OK
