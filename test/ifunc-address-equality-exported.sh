#!/bin/bash
. $(dirname $0)/common.inc

# Clang miscompiles the test code, so skip it if Clang.
# https://github.com/llvm/llvm-project/issues/111338
$CC --version | grep -q clang && skip

supports_ifunc || skip

cat <<EOF | $CC -c -fPIC -o $t/a.o -xc -
typedef void Func();
void foo();
Func *get_foo() { return foo; }
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -c -fno-PIE -o $t/c.o -xc -
#include <stdio.h>

typedef void Func();

__attribute__((ifunc("resolve_foo"))) void foo(void);
void real_foo(void) { printf("foo "); }
Func *resolve_foo() { return real_foo; }

Func *get_foo();

int main() {
  printf("%p %p\n", foo, get_foo());
  foo();
  printf("\n");
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so -no-pie
$QEMU $t/exe | grep -Eq '^(\S+) \1'
