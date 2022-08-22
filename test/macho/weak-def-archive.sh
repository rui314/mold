#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo() { return 3; }
int bar() { return 5; }
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

int foo() __attribute__((weak));
int foo() { return 42; }

int main() {
  printf("foo=%d\n", foo());
}
EOF

clang --ld-path=./ld64 -o $t/exe1 $t/b.a $t/c.o
$t/exe1 | grep -q '^foo=42$'

cat <<EOF | $CC -c -o $t/d.o -xc -
#include <stdio.h>

int foo() __attribute__((weak));
int foo() { return 42; }
int bar();

int main() {
  printf("foo=%d bar=%d\n", foo(), bar());
}
EOF

clang --ld-path=./ld64 -o $t/exe2 $t/b.a $t/d.o
$t/exe2 | grep -q '^foo=3 bar=5$'

echo OK
