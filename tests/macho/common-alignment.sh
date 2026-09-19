#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -fcommon -c -xc -
int foo;
__attribute__((aligned(4096))) int bar;
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
#include <stdint.h>
extern int foo;
extern int bar;
int main() {
  printf("%lu %lu\n", (uintptr_t)&foo % 4, (uintptr_t)&bar % 4096);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep '^0 0$'
