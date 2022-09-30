#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo, bar
foo = 0x1000
bar = 0x2000
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 -Wl,-e,foo $t/a.o $t/b.o
readelf -e $t/exe1 > $t/log
grep -q "Entry point address:.*0x1000$" $t/log

$CC -B. -o $t/exe2 -Wl,-e,bar $t/a.o $t/b.o
readelf -e $t/exe2 > $t/log
grep -q "Entry point address:.*0x2000$" $t/log

echo OK
