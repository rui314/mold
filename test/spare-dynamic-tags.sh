#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

echo '.globl main; main:' | cc -o $t/a.o -c -x assembler -

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
readelf --wide --sections $t/exe > $t/log
grep -Pq '\.dynamic.*\b000190\b' $t/log

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-spare-dynamic-tags=100
readelf --wide --sections $t/exe > $t/log
grep -Pq '\.dynamic.*\b002010\b' $t/log

echo OK
