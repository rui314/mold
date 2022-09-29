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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo, bar
foo:
  .quad 0
bar:
  .quad 0
EOF

./mold -z separate-loadable-segments -e foo -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*0x201000" $t/log

./mold -z separate-loadable-segments -e bar -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*0x201008" $t/log

./mold -z separate-loadable-segments -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*0x201000" $t/log

echo OK
