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

[ $MACHINE = riscv32 ] && { echo skipped; exit; }
[ $MACHINE = riscv64 ] && { echo skipped; exit; }
[ $MACHINE = sparc64 ] && { echo skipped; exit; }

command -v gdb >& /dev/null || { echo skipped; exit; }

echo 'int main() {}' | $CC -o /dev/null -xc -gdwarf-3 -g - >& /dev/null ||
  { echo skipped; exit; }

cat <<EOF | $CC -c -o $t/a.o -fPIC -g -ggnu-pubnames -gdwarf-3 -xc - -ffunction-sections
void hello2();

static void hello() {
  hello2();
}

void greet() {
  hello();
}
EOF

cat <<EOF | $CC -c -o $t/b.o -fPIC -g -ggnu-pubnames -gdwarf-3 -xc - -ffunction-sections
#include <stdio.h>

void trap() {}

void hello2() {
  printf("Hello world\n");
  trap();
}
EOF

$CC -B. -shared -o $t/c.so $t/a.o $t/b.o -Wl,--gdb-index
readelf -WS $t/c.so 2> /dev/null | grep -Fq .gdb_index

cat <<EOF | $CC -c -o $t/d.o -fPIC -g -ggnu-pubnames -gdwarf-3 -xc - -gz
void greet();

int main() {
  greet();
}
EOF

$CC -B. -o $t/exe $t/c.so $t/d.o -Wl,--gdb-index
readelf -WS $t/exe 2> /dev/null | grep -Fq .gdb_index

$QEMU $t/exe | grep -q 'Hello world'

DEBUGINFOD_URLS= gdb $t/exe -nx -batch -ex 'b main' -ex r -ex 'b trap' \
  -ex c -ex bt -ex quit >& $t/log

grep -q 'hello2 () at .*<stdin>:7' $t/log
grep -q 'hello () at .*<stdin>:4' $t/log
grep -q 'greet () at .*<stdin>:8' $t/log
grep -q 'main () at .*<stdin>:4' $t/log

echo OK
