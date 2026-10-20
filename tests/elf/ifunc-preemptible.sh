#!/usr/bin/env bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
#include <stdio.h>

typedef void Func(void);

__attribute__((ifunc("resolve_foo")))
void foo(void);

static void real_foo(void) {
  printf("dso ");
}

static Func *resolve_foo(void) {
  return real_foo;
}

void call_foo(void) { foo(); }
Func *get_foo(void) { return foo; }
Func *foo_ptr = foo;
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

typedef void Func(void);

void foo(void) {
  printf("exe ");
}

void call_foo(void);
Func *get_foo(void);
extern Func *foo_ptr;

int main() {
  call_foo();
  printf("%d %d\n", get_foo() == foo, foo_ptr == foo);
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep '^exe 1 1$'
