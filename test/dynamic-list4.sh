#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>

void foo() { printf("foo1 "); }
void bar() { printf("bar1 "); }
void baz() { printf("baz1 "); }

void print() {
  foo();
  bar();
  baz();
  printf("\n");
}
EOF

cat <<EOF > $t/dyn
{ foo; bar; };
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,--dynamic-list=$t/dyn

cat <<EOF | $CC -o $t/c.o -c -xc - -fPIC
#include <stdio.h>
void foo() { printf("foo2 "); }
void bar() { printf("bar2 "); }
void baz() { printf("baz2 "); }
EOF

$CC -B. -shared -o $t/d.so $t/c.o

cat <<EOF | $CC -o $t/e.o -c -xc -
#include <stdio.h>
void print();
int main() { print(); }
EOF

$CC -B. -o $t/exe1 $t/e.o -Wl,-push-state,-no-as-needed $t/b.so -Wl,-pop-state
$QEMU $t/exe1 | grep -q 'foo1 bar1 baz1'

$CC -B. -o $t/exe2 $t/e.o -Wl,-push-state,-no-as-needed $t/d.so $t/b.so -Wl,-pop-state
$QEMU $t/exe2 | grep -q 'foo2 bar2 baz1'
