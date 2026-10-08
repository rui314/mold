#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Compilers emit a call to a static function in another section as a
# relocation against a section symbol plus an offset. Test that such a
# call reaches the function even if it goes through a range extension
# thunk.

# Skip if 32 bits as we use very large addresses in this test.
[ $MACHINE = i686 ] && skip
[ $MACHINE = riscv32 ] && skip
[ $MACHINE = m68k ] && skip

# It looks like SPARC's runtime can't handle PLT if it's too far from GOT.
[ $MACHINE = sparc64 ] && skip

# LoongArch compilers emit BL, which mold doesn't extend with thunks.
[[ $MACHINE = loongarch* ]] && skip

# ARM32 stores addends in instructions, and mold reports an error for
# such a call if it needs a thunk.
[[ $MACHINE = arm* ]] && skip

# qemu aborts with the "Unknown exception 0x5" error, although this
# test passes on a real POWER10 machine.
on_qemu && [ "$CPU" = power10 ] && skip

cat <<EOF > $t/a.c
#include <stdio.h>

__attribute__((section(".low"), noinline)) static void fn1() { printf(" fn1"); }
__attribute__((section(".low"), noinline)) static void fn2() { printf(" fn2"); }

__attribute__((section(".high"))) void fn3() {
  printf(" fn3");
  fn1();
  fn2();
}

int main() {
  printf(" main");
  fn3();
  printf("\n");
}
EOF

$CC -c -o $t/b.o $t/a.c -O0
$CC -B. -o $t/exe1 $t/b.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
$QEMU $t/exe1 | grep 'main fn3 fn1 fn2'

$CC -c -o $t/c.o $t/a.c -O2
$CC -B. -o $t/exe2 $t/c.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
$QEMU $t/exe2 | grep 'main fn3 fn1 fn2'
