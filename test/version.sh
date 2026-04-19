#!/bin/bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep '__tsan_init' && skip

./mold -v | grep 'mold .*compatible with GNU ld'
./mold --version | grep 'mold .*compatible with GNU ld'

./mold -V | grep 'mold .*compatible with GNU ld'
./mold -V | grep elf_x86_64
./mold -V | grep elf_i386

cat <<EOF | $CC -c -xc -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

rm -f $t/exe
$CC -B. -Wl,--version -o $t/exe1 $t/a.o |& grep mold
not [ -f $t/exe1 ]

$CC -B. -Wl,-v -o $t/exe2 $t/a.o |& grep mold
$QEMU $t/exe2 | grep 'Hello world'

not ./mold --v |& grep 'unknown command line option:'
