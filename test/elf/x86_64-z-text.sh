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

# Skip if libc is musl
ldd --help 2>&1 | grep -q musl && { echo skipped; exit; }

# Skip if target is not x86-64
[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<'EOF' | $CC -c -o $t/a.o -x assembler -
.globl fn1
fn1:
  sub $8, %rsp
  movabs ptr, %rax
  call *%rax
  add $8, %rsp
  ret
EOF

cat <<EOF | $CC -c -o $t/b.o -fPIC -xc -
#include <stdio.h>

int fn1();

int fn2() {
  return 3;
}

void *ptr = fn2;

int main() {
  printf("%d\n", fn1());
}
EOF

$CC -B. -pie -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 3

readelf --dynamic $t/exe | grep -Fq '(TEXTREL)'
readelf --dynamic $t/exe | grep -q '\(FLAGS\).*TEXTREL'

echo OK
