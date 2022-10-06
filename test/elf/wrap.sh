#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

void foo() {
  printf("foo\n");
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

void foo();

void __wrap_foo() {
  printf("wrap_foo\n");
}

int main() {
  foo();
}
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

void __real_foo();

int main() {
  __real_foo();
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q '^foo$'

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-wrap,foo
$QEMU $t/exe | grep -q '^wrap_foo$'

$CC -B. -o $t/exe $t/a.o $t/c.o -Wl,-wrap,foo
$QEMU $t/exe | grep -q '^foo$'
