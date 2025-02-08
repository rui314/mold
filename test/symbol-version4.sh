#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>

void foo() { printf("foo "); }
void foo2() {}
void foo3() {}

__asm__(".symver foo2, foo@TEST2");
__asm__(".symver foo3, foo@TEST3");
EOF

cat <<EOF > $t/b.version
TEST1 { global: foo; };
TEST2 {};
TEST3 {};
EOF

$CC -B. -o $t/c.so -shared $t/a.o -Wl,--version-script=$t/b.version

cat <<EOF | $CC -o $t/d.o -c -xc - -fPIC
#include <stdio.h>

void foo();

void bar() { printf("bar "); }
void bar2() { foo(); }
void bar3() {}

__asm__(".symver bar2, bar@TEST2");
__asm__(".symver bar3, bar@TEST3");
EOF

cat <<EOF > $t/e.version
TEST1 { global: bar; };
TEST2 {};
TEST3 {};
EOF

$CC -B. -o $t/f.so -shared $t/d.o $t/c.so -Wl,--version-script=$t/e.version

cat <<EOF | $CC -o $t/g.o -c -xc -
#include <stdio.h>

void foo();
void bar();

int main() {
  foo();
  bar();
  printf("\n");
}
EOF

$CC -B. -o $t/exe $t/g.o $t/f.so $t/c.so
$QEMU $t/exe | grep 'foo bar'
