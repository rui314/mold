#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A statically-linked executable that exports symbols has .dynamic and
# .dynsym.
test_cflags -static -no-pie || skip

# FreeBSD's libc assumes that a program is dynamically linked if _DYNAMIC
# is defined, so it doesn't support a static executable with .dynamic.
[ "$(uname)" = FreeBSD ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void foo() {}
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -static -no-pie -Wl,--export-dynamic
readelf -W --dyn-syms $t/exe1 | grep -w foo
$QEMU $t/exe1 | grep 'Hello world'

echo '{ foo; };' > $t/dyn
$CC -B. -o $t/exe2 $t/a.o -static -no-pie -Wl,--dynamic-list=$t/dyn
readelf -W --dyn-syms $t/exe2 | grep -w foo
$QEMU $t/exe2 | grep 'Hello world'

$CC -B. -o $t/exe3 $t/a.o -static -no-pie
readelf -WS $t/exe3 | not grep -F .dynamic
