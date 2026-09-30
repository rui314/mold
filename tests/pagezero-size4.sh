#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime rounds -pagezero_size up to a page, with a warning, and caps
# it at 4 GiB in an executable with chained fixups.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
EOF

if [ $ARCH = arm64 ]; then page=0x4000; else page=0x1000; fi
pagezero() { otool -l $1 | awk '$1 == "segname" && $2 == "__PAGEZERO" { getline; getline; print $2; exit }'; }
text() { otool -l $1 | awk '$1 == "segname" && $2 == "__TEXT" { getline; print $2; exit }'; }
hex() { printf '%#x' $(($1)); }

$mold -arch $ARCH -static -e _main $t/a.o -pagezero_size 0x800 -o $t/exe 2> $t/log
grep -q -- "-pagezero_size not aligned, rounded up to: $page, use -segalign to change the alignment" $t/log
[ $(hex $(pagezero $t/exe)) = $page ]
[ $(hex $(text $t/exe)) = $page ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-pagezero_size,0x800 2> $t/log2
grep -q -- "-pagezero_size not aligned, rounded up to: $page" $t/log2
[ $(hex $(pagezero $t/exe2)) = $page ]

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-pagezero_size,0x200000000 2> $t/log3
grep -q -- '-pagezero_size is too large, setting it to 4GB' $t/log3
[ $(hex $(pagezero $t/exe3)) = 0x100000000 ]

$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-pagezero_size,0x200000000 -Wl,-no_fixup_chains
[ $(hex $(pagezero $t/exe4)) = 0x200000000 ]
