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

command -v dwarfdump >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CXX -c -o $t/a.o -g -xc++ -
#include <iostream>
struct Foo {
  Foo() { std::cout << "Hello world\n"; }
};
Foo x;
EOF

cat <<EOF | $CXX -c -o $t/b.o -g -xc++ -
#include <iostream>
struct Foo {
  Foo() { std::cout << "Hello world\n"; }
};
Foo y;
EOF

cat <<EOF | $CXX -o $t/c.o -c -xc++ -g -
int main() {}
EOF

$CXX -B. -o $t/exe $t/a.o $t/b.o $t/c.o -g
$QEMU $t/exe | grep -q 'Hello world'

dwarfdump $t/exe > /dev/null

echo OK
