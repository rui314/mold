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

[ $MACHINE = $(uname -m) ] || { echo skipped; exit; }
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && { echo skipped; exit; }

which gdb >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -c -o $t/a.o -fPIC -g -ggnu-pubnames -gdwarf-4 -xc - -ffunction-sections
#include <stdio.h>

void trap() {}

static void hello() {
  printf("Hello world\n");
  trap();
}

void greet() {
  hello();
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,--gdb-index -Wl,--compress-debug-sections=zlib-gabi
readelf -WS $t/b.so 2> /dev/null | grep -Fq .gdb_index

cat <<EOF | $CC -c -o $t/c.o -fPIC -g -ggnu-pubnames -gdwarf-4 -xc - -gz
void greet();

int main() {
  greet();
}
EOF

$CC -B. -o $t/exe $t/b.so $t/c.o -Wl,--gdb-index -Wl,--compress-debug-sections=zlib-gnu
readelf -WS $t/exe 2> /dev/null | grep -Fq .gdb_index

$QEMU $t/exe | grep -q 'Hello world'

DEBUGINFOD_URLS= gdb $t/exe -nx -batch -ex 'b main' -ex r -ex 'b trap' \
  -ex c -ex bt -ex quit >& $t/log

grep -q 'hello () at .*<stdin>:7' $t/log
grep -q 'greet () at .*<stdin>:11' $t/log
grep -q 'main () at .*<stdin>:4' $t/log

echo OK
