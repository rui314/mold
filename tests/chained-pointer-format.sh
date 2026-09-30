#!/bin/bash
source "$(dirname "$0")"/common.inc

# A chained rebase holds its target as a VM address under
# DYLD_CHAINED_PTR_64 (2) and as an offset from the image's own address
# under DYLD_CHAINED_PTR_64_OFFSET (6), which dyld reads from macOS 12
# on. ld-prime writes 6 for a macOS 12 target, whatever the
# architecture and output kind, and 2 only when -fixup_chains forces
# chains on an older one.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int g = 1;
int *gp = &g;
int main() { printf("%d\n", *gp); }
EOF

fmt() { dyld_info -fixup_chains $1 | grep -m1 -o 'pointer_format: *[0-9]*' | awk '{print $2}'; }

$CC --ld-path=$mold -o $t/exe1 $t/a.o -mmacosx-version-min=11.0 -Wl,-fixup_chains
[ "$(fmt $t/exe1)" = 2 ]
$t/exe1 | grep -q '^1$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -mmacosx-version-min=12.0 -Wl,-fixup_chains
[ "$(fmt $t/exe2)" = 6 ]
$t/exe2 | grep -q '^1$'

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -mmacosx-version-min=12.0
[ "$(fmt $t/b.dylib)" = 6 ]
$CC --ld-path=$mold -o $t/c.bundle -bundle $t/a.o -mmacosx-version-min=12.0
[ "$(fmt $t/c.bundle)" = 6 ]

# The rebase of gp holds g's offset from the image, not its address.
dyld_info -fixup_chain_details $t/exe2 | grep rebase > $t/rebase
g=$(nm $t/exe2 | awk '$3 == "_g" { print $1 }')
text=$(otool -l $t/exe2 | awk '$2 == "__TEXT" { f = 1 } f && $1 == "vmaddr" { print $2; exit }')
off=$(printf '%x' $((0x$g - text)))
grep -q "target: 0x0*$off)" $t/rebase
