#!/usr/bin/env bash
. $(dirname $0)/common.inc

# musl's dynamic linker skips non-default versions such as `foo@VER1`,
# so the executable's reference to `foo` would be left unresolved.
is_musl && skip

# References to `foo` and to its alias `foo@VER1` bind to the exported
# `foo@VER1`, so that the executable's copy of `foo` and its `bar`
# preempt them.

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo = 1;
__asm__(".symver foo, foo@VER1");
int bar() { return 1; }
__asm__(".symver bar, bar@VER1");
int get_foo() { return foo; }
int call_bar() { return bar(); }
EOF

cat <<EOF > $t/b.c
extern int foo_v1;
__asm__(".symver foo_v1, foo@VER1");
int bar_v1();
__asm__(".symver bar_v1, bar@VER1");
int get_foo_v1() { return foo_v1; }
int call_bar_v1() { return bar_v1(); }
EOF

echo 'VER1 {}; VER2 { global: get_*; call_*; local: *; };' > $t/c.ver

cat <<EOF | $CC -fno-PIC -c -o $t/d.o -xc -
#include <stdio.h>
extern int foo;
__asm__(".symver foo, foo@VER1");
int bar() { return 2; }
int get_foo();
int get_foo_v1();
int call_bar();
int call_bar_v1();
int main() {
  foo = 42;
  printf("%d %d %d %d\n", get_foo(), get_foo_v1(), call_bar(), call_bar_v1());
}
EOF

check() {
  $CC -B. -shared -o $t/e.so "$@" -Wl,--version-script=$t/c.ver
  $CC -B. -no-pie -Wl,-E -o $t/exe $t/d.o $t/e.so
  $QEMU $t/exe | grep '^42 42 2 2$'
}

$CC -fPIC -c -o $t/b.o $t/b.c
check $t/a.o $t/b.o

# After GNU ld's -r, as in glibc's libc_pic.os, the references to
# `foo@VER1` refer to its definition.
if test_cflags -fuse-ld=bfd; then
  $CC -fuse-ld=bfd -r -o $t/ab.o $t/a.o $t/b.o
  check $t/ab.o
fi

# With LTO, the references to `foo@VER1` appear only after the first
# symbol resolution.
if test_cflags -flto; then
  $CC -fPIC -flto -c -o $t/b.o $t/b.c
  check -flto $t/a.o $t/b.o
fi
