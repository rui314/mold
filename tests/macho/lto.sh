#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -flto -o $t/a.o -c -xc -
int times2(int x) { return x * 2; }
EOF2

cat <<EOF2 | $CC -flto -o $t/b.o -c -xc -
#include <stdio.h>
int times2(int);
int main() {
  printf("%d\n", times2(21));
}
EOF2

$CC -flto --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep '^42$'

# Mixed bitcode and Mach-O, with bitcode in an archive
cat <<EOF2 | $CC -flto -o $t/c.o -c -xc -
int three() { return 3; }
EOF2
cat <<EOF2 | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int three();
int main() {
  printf("%d\n", three());
}
EOF2
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/c.o
$CC -flto --ld-path=$mold -o $t/exe2 $t/d.o $t/libfoo.a
$t/exe2 | grep '^3$'
