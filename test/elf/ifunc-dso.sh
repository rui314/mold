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

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
void foobar(void);

int main() {
  foobar();
}
EOF

cat <<EOF | $CC -fPIC -shared -o $t/b.so -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
void foobar(void);

static void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

static Func *resolve_foobar(void) {
  return real_foobar;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.so
$t/exe | grep -q 'Hello world'

echo OK
