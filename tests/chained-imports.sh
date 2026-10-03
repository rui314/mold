#!/bin/bash
source "$(dirname "$0")"/common.inc

# A chained-fixups image lists its imports in a table the bind words
# index: one entry per symbol and table addend (addends up to 255 ride
# in the bind word). Each import names itself in a pool after a leading
# NUL, padded to 8 - 8 zero bytes for no imports at all.
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
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern void *a0, *a1[3];
extern char *a2[3];
int main() {
  return !(a0 == (void *)free && a1[0] == (void *)puts && a1[1] == (void *)abort &&
           a1[2] == (void *)strlen && a2[0] == (char *)free + 0x1000 &&
           a2[1] == (char *)free + 0x1000 && a2[2] == (char *)free + 8);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -mmacosx-version-min=13.0
$t/exe
dyld_info -fixup_chain_header $t/exe | sed -n '/targets:/,$p' | \
  awk '$1 == "symbol" { print $2 }' | sort | tr '\n' ' ' > $t/targets
[ "$(cat $t/targets)" = '_abort _free _free _puts _strlen ' ]

echo 'int main() { return 0; }' | $CC -o $t/empty.o -c -xc -
$CC --ld-path=$mold -o $t/exe3 $t/empty.o -mmacosx-version-min=13.0
$t/exe3
symoff=$(dyld_info -fixup_chain_header $t/exe3 | awk '$1 == "symbols_offset" { print $2 }')
size=$(otool -l $t/exe3 | grep -A3 LC_DYLD_CHAINED_FIXUPS | awk '$1 == "datasize" { print $2 }')
[ $((size - symoff)) = 8 ]
