#!/bin/bash
. $(dirname $0)/common.inc

# GCC produces buggy code for this test case on s390x.
# https://sourceware.org/bugzilla/show_bug.cgi?id=29655
[ $MACHINE = s390x ] && $CC -v 2>&1 | grep -E '^gcc version 1[0-3]\.' && skip

# This test does not pass even with GNU ld, which means function pointer
# equality is not guaranteed on HP/PA. I'm not sure if this is an bug in
# QEMU or this test really fails on a real HP/PA machine, but it's quite
# surprising. Many programs could silently break if function pointer
# equality is not guaranteed.
[ $MACHINE = hppa ] && skip

cat <<EOF | $CC -c -fPIC -o$t/a.o -xc -
int foo = 4;

int get_foo() {
  return foo;
}

void *bar() {
  return bar;
}
EOF

$CC -B. -shared -fPIC -o $t/b.so $t/a.o -Wl,-Bsymbolic -Wl,-Bno-symbolic

cat <<EOF | $CC -B. -c -o $t/c.o -xc - -fno-PIE
#include <stdio.h>

extern int foo;
int get_foo();
void *bar();

int main() {
  foo = 3;
  printf("%d %d %d\n", foo, get_foo(), bar == bar());
}
EOF

$CC -B. -no-pie -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep -q '3 3 1'
