#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

# IFUNC is not supported on RISC-V yet
[ "$(uname -m)" = riscv64 ] && { echo skipped; exit; }

# Skip if libc is musl because musl does not support GNU FUNC
echo 'int main() {}' | $CC -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

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

$CC -B. -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
