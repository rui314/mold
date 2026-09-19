#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

char msg[] = "Hello world\n";
char *p = msg;

int main() {
  printf("%s", p);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'
