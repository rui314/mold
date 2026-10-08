#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fcommon -xc -c -o $t/a.o -
int foo[4];
int bar;
int baz[4];
EOF

cat <<EOF | $CC -fcommon -xc -c -o $t/b.o -
#include <stdio.h>
#include <string.h>

extern int baz[4];
int foo[64];
int bar __attribute__((aligned(256)));

int main() {
  memset(foo, 0xff, sizeof(foo));
  printf("%d %lu\n", baz[0], (unsigned long)&bar % 256);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^0 0$'
readelf -sW $t/exe | grep -E ' 256 OBJECT +GLOBAL +DEFAULT +[0-9]+ foo$'

./mold -r -o $t/c.o $t/a.o $t/b.o
readelf -sW $t/c.o | grep -E ' 256 OBJECT +GLOBAL +DEFAULT +COM foo$'
readelf -sW $t/c.o | grep -E ' 0+100 +4 OBJECT +GLOBAL +DEFAULT +COM bar$'
