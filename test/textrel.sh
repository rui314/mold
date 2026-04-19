#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIE
void hello();
int main() { hello(); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fno-PIE
#include <stdio.h>

__attribute__((section(".text")))
int (*fn)(const char *s) = puts;

void hello() {
  puts("Hello world");
}
EOF

$CC -o $t/exe $t/a.o $t/b.o -no-pie
$QEMU $t/exe || skip

$CC -B. -o $t/exe $t/a.o $t/b.o -no-pie
$QEMU $t/exe | grep 'Hello world'
