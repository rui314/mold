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

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo();
int main() {
  foo();
}
EOF

! $CC -B. -o $t/exe $t/a.o 2>&1 \
  | grep -q 'undefined symbol:.*foo'

$CC -B. -o $t/exe $t/a.o -Wl,-warn-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

! $CC -B. -o $t/exe $t/a.o -Wl,-warn-unresolved-symbols \
  --error-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

echo OK
