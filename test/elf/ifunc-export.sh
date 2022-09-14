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

# Skip if libc is musl because musl does not support GNU IFUNC
ldd --help 2>&1 | grep -q musl && { echo skipped; exit; }

cat <<EOF | $CC -c -fPIC -o $t/a.o -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
void foobar(void);

void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

Func *resolve_foobar(void) {
  return real_foobar;
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o
readelf --dyn-syms $t/b.so | grep -Eq '(IFUNC|<OS specific>: 10)\s+GLOBAL DEFAULT   .* foobar'

echo OK
