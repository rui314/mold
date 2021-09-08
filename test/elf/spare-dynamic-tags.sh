#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

echo '.globl main; main:' | cc -o $t/a.o -c -x assembler -

clang -fuse-ld=$mold -o $t/exe $t/a.o
size_before=$((16#$(readelf --wide --sections $t/exe  | grep .dynamic | tr -s ' ' | cut -d ' ' -f7)))

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-spare-dynamic-tags=100
size_after=$((16#$(readelf --wide --sections $t/exe  | grep .dynamic | tr -s ' ' | cut -d ' ' -f7)))

# Ensure space for 95 additional spare tags has been added (default: 5)
[[ $(( $size_after - $size_before )) == $(( 16*95 )) ]]

echo OK
