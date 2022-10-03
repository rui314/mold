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
[ $MACHINE = riscv32 ] && { echo skipped; exit; }
[ $MACHINE = riscv64 ] && { echo skipped; exit; }

# Skip if libc is musl because musl does not support GNU FUNC
ldd --help 2>&1 | grep -q musl && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
static void foobar(void);

static void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

static Func *resolve_foobar(void) {
  return real_foobar;
}

int main() {
  foobar();
}
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,-z,lazy
$QEMU $t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,now
$QEMU $t/exe2 | grep -q 'Hello world'

echo OK
