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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

echo 'int main() {}' | cc -o /dev/null -xc - -static >& /dev/null || \
  { echo skipped; exit; }

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && { echo skipped; exit; }

# Skip if libc is musl because musl does not support GNU IFUNC
ldd --help 2>&1 | grep -q musl && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

void foo() __attribute__((ifunc("resolve_foo")));

void hello() {
  printf("Hello world\n");
}

void *resolve_foo() {
  return hello;
}

int main() {
  foo();
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -static
$QEMU $t/exe | grep -q 'Hello world'

echo OK
