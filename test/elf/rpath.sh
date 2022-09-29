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
  .globl main
main:
  nop
EOF

$CC -B. -o $t/exe $t/a.o \
  -Wl,-rpath,/foo -Wl,-rpath,/bar -Wl,-R/no/such/directory -Wl,-R/

readelf --dynamic $t/exe | \
  grep -Fq 'Library runpath: [/foo:/bar:/no/such/directory:/]'

echo OK
