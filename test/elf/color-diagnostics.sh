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

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { foo(); }
EOF

! ./mold -o $t/exe $t/a.o --color-diagnostics 2> $t/log
! grep -q $'\033' $t/log || false

! ./mold -o $t/exe $t/a.o --color-diagnostics=always 2> $t/log
grep -q $'\033' $t/log

! ./mold -o $t/exe $t/a.o --color-diagnostics=never 2> $t/log
! grep -q $'\033' $t/log || false

! ./mold -o $t/exe $t/a.o --color-diagnostics=auto 2> $t/log
! grep -q $'\033' $t/log || false

echo OK
