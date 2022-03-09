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

rm -f $t/a.o
touch $t/a.o
! $CC -B. -o $t/exe $t/a.o &> $t/log || false
grep -q 'unknown file type: EMPTY' $t/log

echo OK
