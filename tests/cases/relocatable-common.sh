#!/usr/bin/env bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep '__tsan_init' && skip

cat <<EOF | $CC -fcommon -c -o $t/a.o -xc -
int foo;
int bar[8];
EOF

cat <<EOF | $CC -fcommon -c -o $t/b.o -xc -
#include <stdio.h>
extern int foo;
extern int bar[];

int main() {
  foo = 3;
  bar[7] = 5;
  printf("%d %d\n", foo, bar[7]);
}
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o
readelf --symbols $t/c.o > $t/log
grep -E 'OBJECT +GLOBAL +DEFAULT +COM +foo$' $t/log
grep -E 'OBJECT +GLOBAL +DEFAULT +COM +bar$' $t/log

$CC -B. -o $t/exe $t/c.o
$QEMU $t/exe | grep '^3 5$'
