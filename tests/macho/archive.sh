#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int three() { return 3; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int five() { return 5; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int three();
int main() {
  printf("%d\n", three());
}
EOF

rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/a.o $t/b.o

$CC --ld-path=$mold -o $t/exe $t/c.o $t/libfoo.a
$t/exe | grep '^3$'

# An unneeded member should not be linked in
nm $t/exe > $t/syms
grep -q _three $t/syms
not grep -q _five $t/syms
