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
  .text
  .globl foo
  .hidden foo
foo:
  nop
  .globl bar
bar:
  nop
  .globl _start
_start:
  nop
EOF

$CC -shared -fPIC -o $t/b.so -xc /dev/null
./mold -o $t/exe $t/a.o $t/b.so --export-dynamic

readelf --dyn-syms $t/exe > $t/log
grep -Eq 'NOTYPE  GLOBAL DEFAULT    [0-9]+ bar' $t/log
grep -Eq 'NOTYPE  GLOBAL DEFAULT    [0-9]+ _start' $t/log

echo OK
