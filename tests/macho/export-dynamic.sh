#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -flto
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}

int main() {
  hello();
}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -flto -Wl,-no_fixup_chains
$t/exe1 | grep -q 'Hello world'
nm -g $t/exe1 > $t/log1
! grep -q _hello $t/log1 || false

$CC --ld-path=$mold -o $t/exe2 $t/a.o -flto -Wl,-no_fixup_chains -Wl,-export_dynamic
$t/exe2 | grep -q 'Hello world'
nm -g $t/exe2 > $t/log2
grep -q _hello $t/log2
