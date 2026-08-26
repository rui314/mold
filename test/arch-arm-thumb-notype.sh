#!/usr/bin/env bash
. $(dirname $0)/common.inc

test_cflags -mthumb || skip

# Only function symbols indicate in their value whether they are Thumb or
# ARM code. A branch to a symbol of another type, such as a plain label in
# hand-written assembly, must not switch the instruction set.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.syntax unified
.thumb
.globl foo
foo:
  bx lr
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -mthumb -O2
#include <stdio.h>
void foo();
__attribute__((noinline)) void bar() { foo(); }
int main() {
  foo();
  bar();
  printf("OK\n");
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep OK

$OBJDUMP -d $t/exe > $t/log
grep -E '\bbl\b.*<foo>' $t/log
grep -E '\bb\.w\b.*<foo>' $t/log
