#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
#include <stdlib.h>
int other_main() {
  printf("other\n");
  exit(0);
}
int main() { return 1; }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-e,_other_main
$t/exe | grep '^other$'
