#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((weak)) int f(int x) { return x * 3 + 1; }
__attribute__((weak)) int g(int x) { return x * 3 + 1; }
__attribute__((weak)) int h(int x) { return x * 5 + 2; }
int main() {
  printf("%d %d %d\n", (void *)f == (void *)g, (void *)f == (void *)h,
         f(1) + g(2) + h(3));
}
EOF2

# Identical weak functions fold by default
$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '^1 0 28$'

# -no_deduplicate keeps them apart
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_deduplicate
$t/exe2 | grep '^0 0 28$'
