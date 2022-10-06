#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip
supports_ifunc || skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>

void foo() __attribute__((ifunc("resolve_foo")));

void hello() {
  printf("Hello world\n");
}

void *resolve_foo() {
  return hello;
}

int main() {
  foo();
  return 0;
}
EOF

# Skip if the system does not support -static-pie
$CC -o $t/exe1 $t/a.o -static-pie >& /dev/null || skip
$QEMU $t/exe1 >& /dev/null || skip

$CC -B. -o $t/exe2 $t/a.o -static-pie
$QEMU $t/exe2 | grep -q 'Hello world'
