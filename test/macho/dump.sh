#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t=out/test/macho/$testname
mkdir -p $t

echo 'int main() {}' | $CC -o $t/exe -xc -
"$mold" -dump $t/exe > $t/log

grep -q 'magic: 0xfeedfacf' $t/log
grep -q 'segname: __PAGEZERO' $t/log

echo OK
