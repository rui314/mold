#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

void *foo() {
  return printf;
}

int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

objdump --macho --bind $t/exe | grep -q _printf

objdump --macho --lazy-bind $t/exe > $t/log
! grep -q _printf $t/log || false
