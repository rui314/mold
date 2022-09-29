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

cat <<EOF | $CC -c -o $t/a.o -xc++ -
int foo(int, int);
int main() {
  foo(3, 4);
}
EOF

! $CC -B. -o $t/exe $t/a.o -Wl,-no-demangle 2> $t/log || false
grep -q 'undefined symbol: _Z3fooii$' $t/log

! $CC -B. -o $t/exe $t/a.o -Wl,-demangle 2> $t/log || false
grep -Eq 'undefined symbol: foo\(int, int\)$' $t/log

! $CC -B. -o $t/exe $t/a.o 2> $t/log || false
grep -Eq 'undefined symbol: foo\(int, int\)$' $t/log

cat <<EOF | $CC -c -o $t/b.o -xc -
extern int Pi;
int main() {
  return Pi;
}
EOF

! $CC -B. -o $t/exe $t/b.o -Wl,-demangle 2> $t/log || false
grep -q 'undefined symbol: Pi$' $t/log

echo OK
