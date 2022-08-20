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
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | clang++ -o $t/a.o -c -xc++ - -flto
#include <iostream>
int main() {
  std::cout << "Hello world\n";
}
EOF

clang++ --ld-path=./ld64 -o $t/exe $t/a.o -flto -dead_strip_dylibs
$t/exe | grep -q 'Hello world'

echo OK