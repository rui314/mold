#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

rm -rf $t/exe.repro $t/exe.repro.tar

$CC -B. -o $t/exe $t/a.o
! [ -f $t/exe.repro.tar ] || false

$CC -B. -o $t/exe $t/a.o -Wl,-repro

tar -C $t -xf $t/exe.repro.tar
tar -C $t -tvf $t/exe.repro.tar | grep -q ' exe.repro/.*/a.o'
grep -q /a.o  $t/exe.repro/response.txt
grep -q mold $t/exe.repro/version.txt

rm -rf $t/exe.repro $t/exe.repro.tar

MOLD_REPRO=1 $CC -B. -o $t/exe $t/a.o
tar -C $t -tvf $t/exe.repro.tar | grep -q ' exe.repro/.*/a.o'
tar -C $t -xf $t/exe.repro.tar
grep -q /a.o  $t/exe.repro/response.txt
grep -q mold $t/exe.repro/version.txt
