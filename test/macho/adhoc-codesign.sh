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

cat <<EOF | $CC -o $t/exe -xc - -Wl,-adhoc_codesign
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$t/exe | fgrep -q 'Hello world'

echo OK
