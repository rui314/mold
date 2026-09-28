#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

# A read-only absolute reference to an imported symbol promotes the
# symbol to a copy relocation or a canonical PLT entry, which fixes its
# address at link time. Writable references to the same symbol must then
# be resolved at link time as well, whichever section is scanned first.
# Each writable reference has an output section of its own, so that some
# of them are likely to be scanned before the read-only one.

cat <<EOF | $CC -o $t/a.so -fPIC -shared -xc -
int foo = 3;
void bar() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fno-PIC
extern int foo;
void bar();
int *const foo1 = &foo;
void (*const bar1)() = bar;
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -fno-PIC
#include <stdio.h>

extern int foo;
void bar();
extern int *const foo1;
extern void (*const bar1)();

#define REF(n) \
  __attribute__((section("rw" #n))) int *foo##n = &foo; \
  __attribute__((section("rw" #n))) void (*bar##n)() = bar;

REF(2) REF(3) REF(4) REF(5) REF(6) REF(7) REF(8) REF(9)

int main() {
  printf("%d %d\n", foo1 == foo2, bar1 == bar9);
}
EOF

$CC -B. -no-pie -o $t/exe $t/b.o $t/c.o $t/a.so
$QEMU $t/exe | grep '^1 1$'

readelf -rW $t/exe > $t/log
not grep -F R_X86_64_64 $t/log
