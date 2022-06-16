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

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main();

void print() {
  printf("%d\n", (char *)print < (char *)main);
}

int main() {
  print();
}
EOF

cat <<EOF > $t/order1
_print
_main
EOF

cat <<EOF > $t/order2
_main
_print
EOF

clang --ld-path=./ld64 -o $t/exe1 $t/a.o -Wl,-order_file,$t/order1
$t/exe1 | grep -q '^1$'

clang --ld-path=./ld64 -o $t/exe2 $t/a.o -Wl,-order_file,$t/order2
$t/exe2 | grep -q '^0$'

echo OK
