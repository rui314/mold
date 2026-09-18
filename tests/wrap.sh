#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -shared -o $t/a.so -xc -
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

$CC -B. -o $t/exe $t/a.so $t/b.o
$QEMU $t/exe | grep '^foo$'

$CC -B. -o $t/exe $t/a.so $t/b.o -Wl,-wrap,foo
$QEMU $t/exe | grep '^wrap_foo$'

$CC -B. -o $t/exe $t/a.so $t/c.o -Wl,-wrap,foo
$QEMU $t/exe | grep '^foo$'

# A wrapped symbol may itself have a name that starts with `__real_`.
cat <<EOF | $CC -fPIC -shared -o $t/d.so -xc -
#include <stdio.h>

void __real_bar() {
  printf("real_bar\n");
}
EOF

cat <<EOF | $CC -c -o $t/e.o -xc -
#include <stdio.h>

void __real_bar();

void __wrap___real_bar() {
  printf("wrap_real_bar\n");
}

int main() {
  __real_bar();
}
EOF

$CC -B. -o $t/exe $t/d.so $t/e.o -Wl,-wrap,__real_bar
$QEMU $t/exe | grep '^wrap_real_bar$'
