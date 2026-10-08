#!/usr/bin/env bash
. $(dirname $0)/common.inc

test_cflags -static || skip
supports_ifunc || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

static int real_foo(void) { return 3; }
static void *resolve_foo(void) { return real_foo; }
int foo(void) __attribute__((ifunc("resolve_foo")));

int (*fp)(void) = foo;

int main() {
  printf("%d %d\n", foo(), fp());
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -static
$QEMU $t/exe | grep '^3 3$'
