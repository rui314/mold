#!/bin/bash
source "$(dirname "$0")"/common.inc

# A chained-fixups image lists its imports in a table the bind words
# index. ld-prime numbers an import as it first meets it, walking the
# binds subsection by subsection in address order and, within a
# subsection, from the highest offset down; one entry per symbol and
# table addend (addends up to 255 ride in the bind word). Each import
# names itself in a pool after a leading NUL, repeating a name imported
# twice, padded to 8 - 8 zero bytes for no imports at all.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.p2align 3
.globl _a0
_a0: .quad _free
.globl _a1
_a1: .quad _puts, _abort, _strlen
.globl _a2
_a2: .quad _free + 0x1000, _free + 0x1000, _free + 8
.subsections_via_symbols
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -mmacosx-version-min=13.0
$t/exe
dyld_info -fixup_chain_header $t/exe | sed -n '/targets:/,$p' | \
  awk '$1 == "symbol" { printf "%s ", $2 }' > $t/targets
[ "$(cat $t/targets)" = '_free _strlen _abort _puts _free ' ]

$CC --ld-path=$mold -o $t/exe2 $t/main.o -mmacosx-version-min=13.0
$t/exe2
symoff=$(dyld_info -fixup_chain_header $t/exe2 | awk '$1 == "symbols_offset" { print $2 }')
size=$(otool -l $t/exe2 | grep -A3 LC_DYLD_CHAINED_FIXUPS | awk '$1 == "datasize" { print $2 }')
[ $((size - symoff)) = 8 ]
