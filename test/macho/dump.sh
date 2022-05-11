#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/macho/$testname
mkdir -p $t

echo 'int main() {}' | $CC -o $t/exe -xc -
./ld64 -dump $t/exe > $t/log

grep -q 'magic: 0xfeedfacf' $t/log
grep -q 'segname: __PAGEZERO' $t/log

echo OK
