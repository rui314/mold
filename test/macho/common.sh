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
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t=out/test/macho/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -fcommon -c -xc -
int foo;
int bar;
EOF

cat <<EOF | $CC -o $t/b.o -fcommon -c -xc -
int foo;
int bar = 5;
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;
static int baz[10000];

int main() {
  printf("%d %d %d\n", foo, bar, baz[0]);
}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o $t/b.o $t/c.o
$QEMU $t/exe | grep -q '^0 5 0$'

echo OK
