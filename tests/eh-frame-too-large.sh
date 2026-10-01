#!/bin/bash
source "$(dirname "$0")"/common.inc

# An __unwind_info entry in DWARF mode has 24 bits for the offset of
# the function's FDE in __eh_frame. ld-prime gives an FDE beyond their
# reach offset 0 - the unwinder then looks for it through the whole
# section - keeps such entries apart and out of the common encodings,
# and warns, unless -no_warn_eh_frame_too_large. Here a CIE padded with
# 16 MiB of DW_CFA_nops puts the FDEs of _f and _g past it.
if [ $ARCH = arm64 ]; then
  ret=ret ra=30 sp='0x0c, 31, 0' dwarf=3
else
  ret=retq ra=16 sp='0x0c, 7, 8' dwarf=4
fi
fde() {
  printf '%s\n' "L$1:" '.long 24' ".long L$1 + 4 - EH_frame0" "_$1 - ." '.quad 1' '.long 0' |
    sed 's/^_/.quad _/'
}
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main, _f, _g
.p2align 2
_main:
  $ret
_f:
  $ret
_g:
  $ret
.section __TEXT,__eh_frame
EH_frame0:
.long 20 + 0x1000000
.long 0
.byte 1, 0x7a, 0x52, 0, 1, 0x78, $ra, 1, 0x10, $sp, 0, 0, 0, 0
.space 0x1000000
$(fde f)
$(fde g)
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log
grep -Fq 'warning: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected' $t/log
objdump --unwind-info $t/exe > $t/unwind
[ "$(grep -c "encoding\[[0-9]*\]=0x0${dwarf}000000$" $t/unwind)" = 2 ]
grep -q 'Common encodings: (count = 0)' $t/unwind

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_warn_eh_frame_too_large 2> $t/log2
not grep -q __eh_frame $t/log2
