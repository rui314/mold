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

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void say() {
  printf("Hello\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
void say() {
  printf("Howdy\n");
}
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
void say();
int main() {
  say();
}
EOF

mkdir -p $t/x $t/y

ar rcs $t/x/libfoo.a $t/a.o
$CC -shared -o $t/y/libfoo.dylib $t/b.o

clang --ld-path=./ld64 -o $t/exe $t/c.o -Wl,-L$t/x -Wl,-L$t/y -lfoo
$t/exe | grep -q Hello

clang --ld-path=./ld64 -o $t/exe $t/c.o -Wl,-L$t/x -Wl,-L$t/y -lfoo \
 -Wl,-search_paths_first
$t/exe | grep -q Hello

echo OK
