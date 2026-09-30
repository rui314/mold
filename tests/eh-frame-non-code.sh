#!/bin/bash
source "$(dirname "$0")"/common.inc

# Unwind info describes code. ld-prime takes __TEXT,__text and any
# section with the pure_instructions attribute for code, and warns about
# every other section some symbol of which has unwind info, compact or
# DWARF, once a section. An FDE for a function in a section it knows for
# data - by the section's type, as C strings, or by its name, as
# __DATA,__data - is then an error; one in a section of no kind it
# knows is carried into __eh_frame, but gives the function no entry of
# __unwind_info.
if [ $ARCH = arm64 ]; then
  ra=30 sp=31
else
  ra=16 sp=7
fi

# obj <name> <section directive for _f>
obj() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
$2
.globl _f
.p2align 3
_f:
  .quad 0
.section __TEXT,__eh_frame,coalesced,no_toc+strip_static_syms+live_support
EH_frame0:
  .long Lcie_end - Lcie_start
Lcie_start:
  .long 0
  .byte 1
  .asciz "zR"
  .byte 1, 0x78, $ra
  .byte 1, 0x10
  .byte 0x0c, $sp, 8
  .p2align 3
Lcie_end:
_f.eh:
  .long Lfde_end - Lfde_start
Lfde_start:
  .long Lfde_start - EH_frame0
  .quad _f - .
  .quad 8
  .byte 0
  .p2align 3
Lfde_end:
.subsections_via_symbols
EOF
}

# warned <log> <segment,section> <object>: ld-prime names the object by
# its real path.
warned() {
  grep -F "symbols in $2 (" $1 | grep -Fq "$3) have unwind information, but it's not a code section (missing 'regular,pure_instructions' section flag)"
}

# refused <name>: the link fails, in a final link as in a -r one.
refused() {
  not $CC --ld-path=$mold -o $t/$1 $t/$1.o 2> $t/$1.log &&
    grep "invalid function target for dwarf unwind in '" $t/$1.log | grep -Fq "$t/$1.o'" &&
    not $mold -r -arch $ARCH -o $t/$1-r.o $t/$1.o 2> $t/$1-r.log &&
    grep -Fq 'invalid function target for dwarf unwind' $t/$1-r.log
}

obj data .data
refused data
warned $t/data.log __DATA,__data $t/data.o
warned $t/data-r.log __DATA,__data $t/data.o

obj const '.section __TEXT,__const'
refused const
obj cstring '.section __TEXT,__cstring,cstring_literals'
refused cstring
obj pure '.section __DATA,__data,regular,pure_instructions'
refused pure

obj bar '.section __DATA,__bar'
$CC --ld-path=$mold -o $t/bar $t/bar.o 2> $t/bar.log
warned $t/bar.log __DATA,__bar $t/bar.o
f=$(nm $t/bar | awk '$3 == "_f" { print $1 }' | sed 's/^0*//')
dwarfdump --eh-frame $t/bar | grep -q "FDE cie=.* pc=0*$f\.\.\."
objdump --unwind-info $t/bar > $t/bar.unwind
not grep -qi "function offset=0x0*${f: -5}," $t/bar.unwind
# The FDE still gets the image an __unwind_info, which lists the code:
# _main, with no unwind info of its own (encoding 0).
m=$(nm $t/bar | awk '$3 == "_main" { print $1 }' | sed 's/^0*//')
grep -qi "function offset=0x0*${m: -5}, encoding.*=0x00000000" $t/bar.unwind
$mold -r -arch $ARCH -o $t/bar-r.o $t/bar.o 2> $t/bar-r.log
warned $t/bar-r.log __DATA,__bar $t/bar.o
dwarfdump --eh-frame $t/bar-r.o | grep -q 'FDE cie='

# A section with no kind of its own and no code, the functions of which
# have compact unwind: the same warning, once.
cat <<EOF | $CC -o $t/compact.o -c -xassembler -
.section __DATA,__bar
.globl _g
.p2align 2
_g:
  .cfi_startproc
  ret
  .cfi_endproc
.globl _h
.p2align 2
_h:
  .cfi_startproc
  ret
  .cfi_endproc
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/compact-r.o $t/compact.o 2> $t/compact.log
warned $t/compact.log __DATA,__bar $t/compact.o
[ "$(grep -c 'unwind information' $t/compact.log)" = 1 ]
