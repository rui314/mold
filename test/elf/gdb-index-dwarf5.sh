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

[ $MACHINE = $(uname -m) ] || { echo skipped; exit; }

[ $MACHINE = riscv32 ] && { echo skipped; exit; }
[ $MACHINE = riscv64 ] && { echo skipped; exit; }
[ $MACHINE = sparc64 ] && { echo skipped; exit; }

command -v gdb >& /dev/null || { echo skipped; exit; }

echo 'int main() {}' | $CC -o /dev/null -xc -gdwarf-5 -g - >& /dev/null ||
  { echo skipped; exit; }

cat <<EOF > $t/a.c
void fn3();

static void fn2() {
  fn3();
}

void fn1() {
  fn2();
}
EOF

cat <<EOF > $t/b.c
void fn5();

static void fn4() {
  fn5();
}

void fn3() {
  fn4();
}
EOF

cat <<EOF > $t/c.c
void fn7();

static void fn6() {
  fn7();
}

void fn5() {
  fn6();
}
EOF

cat <<EOF > $t/d.c
#include <stdio.h>
void trap() {}

static void fn8() {
  printf("Hello world\n");
  trap();
}

void fn7() {
  fn8();
}
EOF

$CC -c -o $t/a.o $t/a.c -fPIC -g -ggnu-pubnames -gdwarf-5 -ffunction-sections
$CC -c -o $t/b.o $t/b.c -fPIC -g -ggnu-pubnames -gdwarf-4 -ffunction-sections
$CC -c -o $t/c.o $t/c.c -fPIC -g -ggnu-pubnames -gdwarf-5
$CC -c -o $t/d.o $t/d.c -fPIC -g -ggnu-pubnames -gdwarf-5 -ffunction-sections

$CC -B. -shared -o $t/e.so $t/a.o $t/b.o $t/c.o $t/d.o -Wl,--gdb-index
readelf -WS $t/e.so 2> /dev/null | grep -Fq .gdb_index

cat <<EOF | $CC -c -o $t/f.o -fPIC -g -ggnu-pubnames -gdwarf-5 -xc - -gz
void fn1();

int main() {
  fn1();
}
EOF

$CC -B. -o $t/exe $t/e.so $t/f.o -Wl,--gdb-index
readelf -WS $t/exe 2> /dev/null | grep -Fq .gdb_index

$QEMU $t/exe | grep -q 'Hello world'

DEBUGINFOD_URLS= gdb $t/exe -nx -batch -ex 'b main' -ex r -ex 'b trap' \
  -ex c -ex bt -ex quit >& $t/log

grep -q 'fn8 () at .*/d.c:6' $t/log
grep -q 'fn7 () at .*/d.c:10' $t/log
grep -q 'fn6 () at .*/c.c:4' $t/log
grep -q 'fn5 () at .*/c.c:8' $t/log
grep -q 'fn4 () at .*/b.c:4' $t/log
grep -q 'fn3 () at .*/b.c:8' $t/log
grep -q 'fn2 () at .*/a.c:4' $t/log
grep -q 'fn1 () at .*/a.c:8' $t/log

echo OK
