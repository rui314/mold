#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
char msg[] = "Hello world\n";
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

extern char msg[];

int main() {
  printf("%s\n", msg);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'
