#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.c
#include <stdio.h>

void fn3();
void fn4();

__attribute__((section(SEC1))) void fn1() { printf(" fn1"); fn3(); }
__attribute__((section(SEC1))) void fn2() { printf(" fn2"); fn4(); }

int main() {
  printf(" main");
  fn1();
  printf("\n");
}
EOF

cat <<EOF > $t/b.c
#include <stdio.h>

void fn1();
void fn2();

__attribute__((section(SEC2))) void fn3() { printf(" fn3"); fn2(); }
__attribute__((section(SEC2))) void fn4() { printf(" fn4"); }
EOF

# .high is placed billions of bytes away from the rest of the program.
# Functions there cannot have unwind tables because an FDE refers to its
# function with a 32-bit PC-relative relocation. We put fn1 and fn2 in
# either section to test calls in both directions.
flags="-fno-asynchronous-unwind-tables -fno-unwind-tables"
starts="-Wl,--section-start=.low=0x10000000,--section-start=.high=0x100000000"

$CC -c -o $t/c.o $t/a.c $flags -DSEC1='".low"' -DSEC2='".high"'
$CC -c -o $t/d.o $t/b.c $flags -DSEC1='".low"' -DSEC2='".high"'
$CC -B. -o $t/exe1 $t/c.o $t/d.o $starts
$QEMU $t/exe1 | grep 'main fn1 fn3 fn2 fn4'

$CC -c -o $t/e.o $t/a.c $flags -DSEC1='".high"' -DSEC2='".low"'
$CC -c -o $t/f.o $t/b.c $flags -DSEC1='".high"' -DSEC2='".low"'
$CC -B. -o $t/exe2 $t/e.o $t/f.o $starts
$QEMU $t/exe2 | grep 'main fn1 fn3 fn2 fn4'

flags="$flags -fno-PIC -mcmodel=large"
starts="-Wl,--section-start=.low=0x10000000,--section-start=.high=0x400000000"

$CC -c -o $t/g.o $t/a.c $flags -DSEC1='".low"' -DSEC2='".high"'
$CC -c -o $t/h.o $t/b.c $flags -DSEC1='".low"' -DSEC2='".high"'
$CC -B. -o $t/exe3 $t/g.o $t/h.o -pie $starts
$QEMU $t/exe3 | grep 'main fn1 fn3 fn2 fn4'

$CC -c -o $t/i.o $t/a.c $flags -DSEC1='".high"' -DSEC2='".low"'
$CC -c -o $t/j.o $t/b.c $flags -DSEC1='".high"' -DSEC2='".low"'
$CC -B. -o $t/exe4 $t/i.o $t/j.o -pie $starts
$QEMU $t/exe4 | grep 'main fn1 fn3 fn2 fn4'
