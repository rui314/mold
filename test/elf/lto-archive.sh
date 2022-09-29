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

[ "$CC" = cc ] || { echo skipped; exit; }

echo 'int main() {}' | $CC -flto -o /dev/null -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -flto -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -flto -xc -
#include <stdio.h>
void howdy() {
  printf("Hello world\n");
}
EOF

rm -f $t/c.a
ar rc $t/c.a $t/a.o $t/b.o

cat <<EOF | $CC -o $t/d.o -c -flto -xc -
void hello();
int main() {
  hello();
}
EOF

$CC -B. -o $t/exe -flto $t/d.o $t/c.a
$QEMU $t/exe | grep -q 'Hello world'

nm $t/exe > $t/log
grep -q hello $t/log
! grep -q howdy $t/log || false

echo OK
