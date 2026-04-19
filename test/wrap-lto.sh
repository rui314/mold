#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -flto || skip

cat <<EOF | $CC -fPIC -shared -o $t/a.so -xc -
#include <stdio.h>

void foo() {
  printf("foo\n");
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc - -flto
#include <stdio.h>

void foo();

void __wrap_foo() {
  printf("wrap_foo\n");
}

int main() {
  foo();
}
EOF

cat <<EOF | $CC -c -o $t/c.o -xc - -flto
#include <stdio.h>

void __real_foo();

int main() {
  __real_foo();
}
EOF

$CC -B. -o $t/exe $t/a.so $t/b.o -flto
$QEMU $t/exe | grep '^foo$'

$CC -B. -o $t/exe $t/a.so $t/b.o -Wl,-wrap,foo -flto
$QEMU $t/exe | grep '^wrap_foo$'

$CC -B. -o $t/exe $t/a.so $t/c.o -Wl,-wrap,foo -flto
$QEMU $t/exe | grep '^foo$'
