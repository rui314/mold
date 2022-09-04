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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

if [ $MACHINE = aarch64 ]; then
  entry_addr_prefix=0x21000
else
  entry_addr_prefix=0x20100
fi

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo, bar
foo:
  .quad 0
bar:
  .quad 0
EOF

./mold -e foo -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*${entry_addr_prefix}0" $t/log

./mold -e bar -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*${entry_addr_prefix}8" $t/log

./mold -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*${entry_addr_prefix}0" $t/log

cat <<EOF > $t/script
ENTRY(bar)
EOF

./mold -static -o $t/exe $t/a.o $t/script
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*${entry_addr_prefix}8" $t/log

./mold -e foo -static -o $t/exe $t/a.o $t/script
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*${entry_addr_prefix}0" $t/log

echo OK
