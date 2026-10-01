#!/bin/bash
source "$(dirname "$0")"/common.inc

[ $ARCH = arm64 ] || skip

# ld-prime's map lists the branch islands among the linker's own
# subsections, as the symbol table names them: after their target. (Its
# islands go between subsections, so the code between the branch and
# its target comes in subsections of 2 MiB.)
{
  echo '.subsections_via_symbols'
  echo '.globl _main'
  echo '.p2align 2'
  echo '_main:'
  echo '  b _far'
  for i in $(seq 70); do
    echo "_pad$i:"
    echo "  .long $i"
    echo '  .space 0x200000 - 4'
  done
  echo '.globl _far'
  echo '_far:'
  echo '  mov w0, #0'
  echo '  ret'
} | $CC -o $t/a.o -c -xassembler -

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
$t/exe
grep -Eq $'^0x[0-9A-F]+\t0x[0-9A-F]+\t\\[  0\\] _far.island$' $t/map
