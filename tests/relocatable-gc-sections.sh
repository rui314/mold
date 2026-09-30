#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc - -O2
const char *hello() { return "Hello world"; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
const char *hello();
int main() { printf("%s\n", hello()); }
EOF

./mold -r --gc-sections -o $t/c.o $t/a.o
$CC -B. -o $t/exe $t/b.o $t/c.o
$QEMU $t/exe | grep '^Hello world$'
