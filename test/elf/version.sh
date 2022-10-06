#!/bin/bash
. $(dirname $0)/common.inc

./mold -v | grep -q 'mold .*compatible with GNU ld'
./mold --version | grep -q 'mold .*compatible with GNU ld'

./mold -V | grep -q 'mold .*compatible with GNU ld'
./mold -V | grep -q elf_x86_64
./mold -V | grep -q elf_i386

cat <<EOF | $CC -c -xc -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

rm -f $t/exe
$CC -B. -Wl,--version -o $t/exe $t/a.o 2>&1 | grep -q mold
! [ -f $t/exe ] || false

$CC -B. -Wl,-v -o $t/exe $t/a.o 2>&1 | grep -q mold
$QEMU $t/exe | grep -q 'Hello world'

! ./mold --v >& $t/log
grep -q 'unknown command line option:' $t/log
