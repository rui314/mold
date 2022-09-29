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

cat <<EOF > $t/a.h
#define A 23
#define B 99
EOF

cat <<EOF | $GCC -o $t/b.o -c -xc - -I$t -g3
#include "a.h"
extern int z();
int main () { return z() - 122; }
EOF

cat <<EOF | $GCC -o $t/c.o -c -xc - -I$t -g3
#include "a.h"
int z()  { return A + B; }
EOF

$GCC -B. -o $t/exe $t/b.o $t/c.o
${TRIPLE}objdump --dwarf=macro $t/exe > $t/log
! grep 'DW_MACRO_import -.* 0x0$' $t/log || false

echo OK
