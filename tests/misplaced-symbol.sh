#!/bin/bash
source "$(dirname "$0")"/common.inc

# An assembler places a symbol set to an address past its section's end
# in that section all the same. ld-prime ignores such a symbol with a
# warning, and a relocation to it is an error naming its nlist index.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
.globl _bar
_bar = _main + 0x1000
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log
grep -q "_bar symbol is ignored, because its address isn't in its designated section" $t/log
nm $t/exe > $t/nm
not grep -q _bar $t/nm

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
_main:
  ret
.data
.quad _bar
.globl _bar
_bar = _main + 0x1000
EOF

not $CC --ld-path=$mold -o $t/exe $t/b.o 2> $t/log2
grep -Eq "invalid r_symbolnum=[0-9]+, a global symbol at this index is missing in '.*b.o'" $t/log2
