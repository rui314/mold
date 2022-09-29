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

cat <<EOF | $CXX -o $t/a.o -c -xc++ -
#include <iostream>
#include <stdexcept>

class Foo : public std::runtime_error {
public:
  using std::runtime_error::runtime_error;
};

static void do_throw() {
  throw Foo("exception");
}

int main() {
  try {
    do_throw();
  } catch (const Foo &e) {
    std::cout << "error: " << e.what() << std::endl;
  }
}
EOF

$CXX -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep -q 'error: exception'

echo OK
