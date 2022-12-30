#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CXX -o $t/a.o -c -xc++ - -fno-PIE
#include <assert.h>
#include <stdio.h>

__attribute__((target("default")))
int foo() {
  return 0;
}

__attribute__((target("ssse3,avx2")))
int foo() {
  return 1;
}

int (*p)() = foo;

int main() {
  int val = foo();
  assert(val == p());
}
EOF

$CXX -B. -o $t/exe $t/a.o -static
$QEMU $t/exe
