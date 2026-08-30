#!/usr/bin/env bash
. $(dirname $0)/common.inc

case $MACHINE in
i686|arm*|ppc|riscv32*|sh4*|m68k) skip ;;
esac

# An unresolved weak function resolves to address 0. If a program is linked
# more than 4 GiB away from it, no call relocation can encode a call to the
# function, but such a call is guarded by a null check and never executed.
cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>
__attribute__((weak)) void foo(void);
int main() {
  if (foo)
    foo();
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -pie -Wl,-Ttext-segment=0x100000000
$QEMU $t/exe | grep 'Hello world'
