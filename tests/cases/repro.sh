#!/usr/bin/env bash
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
not [ -f $t/exe.repro.tar ]

echo "INPUT($t/a.o)" > $t/script
echo '{ global: main; local: *; };' > $t/version

$CC -B. -o $t/exe $t/script -Wl,--version-script=$t/version,-repro

tar -C $t -xf $t/exe.repro.tar
tar -C $t -tvf $t/exe.repro.tar | grep ' exe.repro/.*/a.o'
tar -C $t -tvf $t/exe.repro.tar | grep ' exe.repro/.*/script'
tar -C $t -tvf $t/exe.repro.tar | grep ' exe.repro/.*/version'
grep /script $t/exe.repro/response.txt
grep mold $t/exe.repro/version.txt

rm -rf $t/exe.repro $t/exe.repro.tar

MOLD_REPRO=1 $CC -B. -o $t/exe $t/a.o
tar -C $t -tvf $t/exe.repro.tar | grep ' exe.repro/.*/a.o'
tar -C $t -xf $t/exe.repro.tar
grep /a.o  $t/exe.repro/response.txt
grep mold $t/exe.repro/version.txt
