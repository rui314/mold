#!/usr/bin/env bash
. $(dirname $0)/common.inc

supports_ifunc || skip

# In a position-dependent executable, the address of an ifunc is that of
# its .plt.got entry, so the entry has to start with endbr64 for IBT.
cat <<EOF | $CC -fno-PIE -c -o $t/a.o -xc -
#include <stdio.h>

static int real_foo(void) { return 3; }
static void *resolve_foo(void) { return real_foo; }
int foo(void) __attribute__((ifunc("resolve_foo")));

int (*fp)(void) = foo;

int main() {
  printf("%d %d\n", foo(), fp());
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.o
$QEMU $t/exe | grep '^3 3$'
$OBJDUMP -d -j .plt.got $t/exe | grep -A1 '<foo\$pltgot>:' | grep -w endbr64
