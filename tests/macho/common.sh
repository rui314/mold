#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -fcommon -c -xc -
int foo;
int bar;
EOF

cat <<EOF | $CC -o $t/b.o -fcommon -c -xc -
int foo;
int bar = 5;
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;
static int baz[10000];

int main() {
  printf("%d %d %d\n", foo, bar, baz[0]);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^0 5 0$'
