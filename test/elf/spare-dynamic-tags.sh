#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

echo '.globl main; main:' | $CC -o $t/a.o -c -x assembler -

$CC -B. -o $t/exe $t/a.o
size_before=$((16#$(readelf --wide --sections $t/exe  | grep .dynamic | tr -s ' ' | cut -d ' ' -f7)))

$CC -B. -o $t/exe $t/a.o -Wl,-spare-dynamic-tags=100
size_after=$((16#$(readelf --wide --sections $t/exe  | grep .dynamic | tr -s ' ' | cut -d ' ' -f7)))

# Ensure space for 95 additional spare tags has been added (default: 5)
[[ $(( $size_after - $size_before )) == $(( 16*95 )) ]]

echo OK
