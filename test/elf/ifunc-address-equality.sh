#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -c -fno-PIE -o $t/a.o -xc -
#include <stdio.h>

typedef void Func();

__attribute__((ifunc("resolve_foo"))) void foo(void);
void real_foo(void) { printf("foo "); }
Func *resolve_foo() { return real_foo; }

__attribute__((ifunc("resolve_bar"))) void bar(void);
void real_bar(void) { printf("bar "); }
Func *resolve_bar() { return real_bar; }
EOF

cat <<EOF | $CC -c -fPIC -o $t/b.o -xc -
typedef void Func();

void foo();
void bar();

Func *get_foo() { return foo; }
Func *get_bar() { return bar; }
EOF

cat <<EOF | $CC -c -fno-PIE -o $t/c.o -xc -
#include <stdio.h>

typedef void Func();

void foo();
void bar();
Func *get_foo();
Func *get_bar();

int main() {
  printf("%p %p %p %p\n", foo, get_foo(), bar, get_bar());
  foo();
  bar();
  printf("\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -no-pie
$QEMU $t/exe1 | grep -Eq '^(\S+) \1 (\S+) \2'

readelf --dynamic $t/exe1 > $t/log1
! grep -q TEXTREL $t/log1 || false
