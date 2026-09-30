#!/bin/bash
source "$(dirname "$0")"/common.inc

# An undefined symbol's n_desc carries its library ordinal in the high
# byte, which only a two-level namespace image has: the dylib's
# load-command ordinal, or DYNAMIC_LOOKUP_ORDINAL (0xfe) for a symbol
# left to -undefined dynamic_lookup or -U. ld-prime writes 0 for every
# import of a -flat_namespace image, chained fixups or not; its binds
# say flat lookup (-2) all the same.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int missing(void);
int main() { printf("%d\n", missing()); }
EOF

desc() { nm -x $1 | awk -v n=$2 '$NF == n { print $4 }'; }

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-undefined,dynamic_lookup
[ "$(desc $t/exe1 _printf)" = 0100 ]
[ "$(desc $t/exe1 _missing)" = fe00 ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-flat_namespace -Wl,-undefined,dynamic_lookup
[ "$(desc $t/exe2 _printf)" = 0000 ]
[ "$(desc $t/exe2 _missing)" = 0000 ]

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-flat_namespace -Wl,-U,_missing -Wl,-fixup_chains
[ "$(desc $t/exe3 _printf)" = 0000 ]
[ "$(desc $t/exe3 _missing)" = 0000 ]
dyld_info -fixups $t/exe3 > $t/fixups3
grep -q 'flat-namespace.*_printf' $t/fixups3
