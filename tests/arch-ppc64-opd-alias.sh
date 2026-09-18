#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A reference through the .opd section symbol must bind to the local
# function, not to one of its global or weak aliases, which a definition
# in another file could preempt.
cat <<EOF | $CC -B. -fPIC -shared -o $t/a.so -xc -
static int impl(int x) { return x + 1; }
int alias_pub(int) __attribute__((alias("impl")));
int alias_weak(int) __attribute__((weak, alias("impl")));

static int (*fp)(int) = impl;
int call_fp(int x) { return fp(x); }
int call_impl(int x) { return impl(x); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

int alias_pub(int x) { return x + 1000; }
int call_fp(int x);
int call_impl(int x);

int main() {
  printf("%d %d\n", call_fp(1), call_impl(1));
}
EOF

$CC -B. -o $t/exe $t/b.o $t/a.so -Wl,-rpath,$t
$QEMU $t/exe | grep '^2 2$'
