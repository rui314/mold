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
cd "$(dirname "$0")"/../..
t=out/test/macho/$testname
mkdir -p $t

# For some reason, this test fails only on GitHub CI.
[ "$GITHUB_ACTIONS" = true ] && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

_Thread_local int a;
static _Thread_local int b = 5;

int main() {
  b = 5;
  printf("%d %d\n", a, b);
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o
$t/exe | grep -q '^0 5$'

echo OK
