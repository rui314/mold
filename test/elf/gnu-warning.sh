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

cat <<EOF | $GCC -c -o $t/a.o -xc -
void foo() {}

__attribute__((section(".gnu.warning.foo")))
static const char foo_warning[] = "warning message";
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo();

int main() { foo(); }
EOF

# Make sure that we do not copy .gnu.warning.* sections.
$CC -B. -o $t/exe $t/a.o $t/b.o
! readelf --sections $t/exe | grep -Fq .gnu.warning || false

echo OK
