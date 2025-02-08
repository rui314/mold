#!/bin/bash
. $(dirname $0)/common.inc

on_qemu && skip
[ $MACHINE = riscv64 -o $MACHINE = riscv32 -o $MACHINE = sparc64 ] && skip

command -v gdb >& /dev/null || skip

test_cflags -gdwarf-5 -g -gdwarf64 || skip

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

$CC -c -o $t/a.o $t/a.c -fPIC -g -ggnu-pubnames -gdwarf-5 -gdwarf64 -ffunction-sections
$CC -c -o $t/b.o $t/b.c -fPIC -g -ggnu-pubnames -gdwarf-4 -gdwarf64 -ffunction-sections
$CC -c -o $t/c.o $t/c.c -fPIC -g -ggnu-pubnames -gdwarf-5 -gdwarf64
$CC -c -o $t/d.o $t/d.c -fPIC -g -ggnu-pubnames -gdwarf-5 -gdwarf64 -ffunction-sections

$CC -B. -shared -o $t/e.so $t/a.o $t/b.o $t/c.o $t/d.o

$CC -B. -shared -o $t/f.so $t/a.o $t/b.o $t/c.o $t/d.o -Wl,--gdb-index
readelf -WS $t/f.so 2> /dev/null | grep -F .gdb_index

cat <<EOF | $CC -c -o $t/g.o -fPIC -g -ggnu-pubnames -gdwarf-5 -xc - -gz
void fn1();

int main() {
  fn1();
}
EOF

# Older versions of gdb are buggy that they complain DWARF64 debug sections
# even without .gdb_index. Skip if such version.
$CC -B. -o $t/exe1 $t/e.so $t/g.o

DEBUGINFOD_URLS= gdb $t/exe1 -nx -batch -ex 'b main' -ex r -ex quit |&
  grep 'DW_FORM_line_strp pointing outside of .debug_line_str' && skip

# We are using a recent version of gdb.
$CC -B. -o $t/exe2 $t/f.so $t/g.o -Wl,--gdb-index
readelf -WS $t/exe2 2> /dev/null | grep -F .gdb_index

$QEMU $t/exe2 | grep 'Hello world'

DEBUGINFOD_URLS= gdb $t/exe2 -nx -batch -ex 'b main' -ex r -ex 'b trap' \
  -ex c -ex bt -ex quit >& $t/log2

grep 'fn8 () at .*/d.c:6' $t/log2
grep 'fn7 () at .*/d.c:10' $t/log2
grep 'fn6 () at .*/c.c:4' $t/log2
grep 'fn5 () at .*/c.c:8' $t/log2
grep 'fn4 () at .*/b.c:4' $t/log2
grep 'fn3 () at .*/b.c:8' $t/log2
grep 'fn2 () at .*/a.c:4' $t/log2
grep 'fn1 () at .*/a.c:8' $t/log2
