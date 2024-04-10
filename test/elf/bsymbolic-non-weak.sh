#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -fPIC -xc -
__attribute__((weak)) int foo = 4;
int bar = 4;
__attribute__((weak)) int baz = 4;

int get_foo1() { return foo; }
int get_bar1() { return bar; }
__attribute__((weak)) int get_baz1() { return baz; }

int get_foo2() { return get_foo1(); }
int get_bar2() { return get_bar1(); }
int get_baz2() { return get_baz1(); }
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-Bsymbolic-non-weak

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

int foo = 3;
int bar = 3;
int baz = 3;

int get_foo1() { return 7; }
int get_bar1() { return 7; }
int get_baz1() { return 7; }

int get_foo2();
int get_bar2();
int get_baz2();

int main() {
  printf("%d %d %d %d %d %d\n", foo, bar, baz,
         get_foo2(), get_bar2(), get_baz2());
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep -q '^3 3 3 3 4 7$'
