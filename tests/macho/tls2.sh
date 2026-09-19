#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
_Thread_local int a;
static _Thread_local int b = 5;
static _Thread_local int *c;
int main() {
  b = 5;
  c = &b;
  printf("%d %d %d\n", a, b, *c);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '^0 5 5$'
