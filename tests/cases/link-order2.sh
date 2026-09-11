#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int bar() { return 3; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int foo() { return 2; }
EOF

cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>

int foo();
int bar();

int main() {
  printf("%d %d\n", foo(), bar());
}
EOF

rm -f $t/e.a
ar rcs $t/e.a $t/a.o $t/b.o

$CC -B. -o $t/exe $t/d.o $t/c.o $t/e.a
$QEMU $t/exe | grep '^2 3$'

# Only b.o should be extracted from e.a: the explicitly linked c.o
# already defines foo, even when the archive precedes it.
$CC -B. -o $t/exe $t/e.a $t/c.o $t/d.o
$QEMU $t/exe | grep '^2 3$'
