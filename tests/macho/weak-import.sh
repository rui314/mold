#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((weak_import)) extern void qsort_r(void *, size_t, size_t, void *,
                                                 int (*)(void *, const void *, const void *));
int main() {
  printf("%d\n", qsort_r != 0);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '^1$'
otool -Sv $t/exe > /dev/null 2>&1 || true
nm -m $t/exe | grep -q 'weak.*_qsort_r'
