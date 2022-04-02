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

cat <<EOF | clang -c -o $t/a.o -xc -
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | clang -c -o $t/b.o -xc -
void foo() {}
EOF

rm -f $t/c.a
ar rcs $t/c.a $t/a.o $t/b.o

cat <<EOF | clang -c -o $t/d.o -xc -
void hello();

int main() {
  hello();
}
EOF

clang++ -fuse-ld="$mold" -o $t/exe $t/d.o $t/c.a
$QEMU $t/exe | grep -q 'Hello world'

otool -tv $t/exe | grep -q '^_hello:'
! otool -tv $t/exe | grep -q '^_foo:' || false

echo OK
