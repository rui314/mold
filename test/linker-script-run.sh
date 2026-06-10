#!/bin/bash
. $(dirname $0)/common.inc

# Range extension thunks are not supported in script layout yet
[[ $MACHINE = ppc* ]] && skip

# An executable laid out by a linker script must actually run. The
# script places the standard sections; everything else (dynamic
# sections, TLS, etc.) is placed as orphans.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((section(".greeting"))) char msg[6] = "Hello";
int main() { printf("%s world\n", msg); }
EOF

cat <<'EOF' > $t/script
SECTIONS {
  . = 0x400000 + SIZEOF_HEADERS;
  .text : { *(.text .text.*) }
  . = ALIGN(CONSTANT(MAXPAGESIZE));
  .rodata : { *(.rodata .rodata.*) }
  .greeting : { KEEP(*(.greeting)) }
  . = ALIGN(CONSTANT(MAXPAGESIZE));
  .data : { *(.data .data.*) }
  .bss : { *(.bss .bss.* COMMON) }
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-T,$t/script -no-pie
$QEMU $t/exe | grep 'Hello world'
