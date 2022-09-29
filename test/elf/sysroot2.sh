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

mkdir -p $t/sysroot/foo

cat <<EOF > $t/a.script
INPUT(=/foo/x.o)
EOF

cat <<EOF > $t/sysroot/b.script
INPUT(/foo/y.o)
EOF

cat <<EOF | $CC -c -o $t/sysroot/foo/x.o -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -c -o $t/sysroot/foo/y.o -xc -
#include <stdio.h>
void hello2() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
void hello();
void hello2();

int main() {
  hello();
  hello2();
}
EOF

$CC -B. -o $t/exe -Wl,--sysroot=$t/sysroot \
  $t/a.script $t/sysroot/b.script $t/c.o

echo OK
