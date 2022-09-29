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

echo '.section foo,"R"' | $CC -o /dev/null -c -xassembler - 2> /dev/null ||
  { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section .text.foo, "aR", @progbits
.globl foo
foo:
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section .text.foo, "a", @progbits
.globl foo
foo:
  ret
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -Wl,-gc-sections
nm $t/exe1 | grep -q foo

$CC -B. -o $t/exe1 $t/a.o $t/c.o -Wl,-gc-sections
nm $t/exe1 > $t/log
! grep -q foo $t/log || false

echo OK
