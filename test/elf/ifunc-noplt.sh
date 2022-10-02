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

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && { echo skipped; exit; }

# Skip if libc is musl because musl does not support GNU FUNC
ldd --help 2>&1 | grep -q musl && { echo skipped; exit; }

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc - -fno-plt
#include <stdio.h>

__attribute__((ifunc("resolve_foo")))
void foo(void);

void hello(void) {
  printf("Hello world\n");
}

typedef void Fn();

Fn *resolve_foo(void) {
  return hello;
}

int main() {
  foo();
}
EOF

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep -q 'Hello world'

echo OK
