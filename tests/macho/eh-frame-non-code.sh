#!/bin/bash
source "$(dirname "$0")"/common.inc

# Unwind info describes code: a section with instructions, or
# __TEXT,__text. Of every other section some symbol of which has unwind
# info, compact or DWARF, the linker warns, once a section; an FDE of a
# function there is carried into __eh_frame, but gives the function no
# entry of __unwind_info. (ld-prime takes a section with
# pure_instructions for code but one it knows for data, and refuses an
# FDE in a section of data, by type or by name, as __DATA,__data.)
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

# warned <log> <segment,section> <object>
warned() {
  grep -F "symbols in $2 (" $1 | grep -Fq "$3) have unwind information, but it's not a code section"
}

# carried <name> <segment,section>: the image keeps _f's FDE, with no
# entry of __unwind_info for _f, and a -r output keeps it too, both with
# the warning.
carried() {
  $CC --ld-path=$mold -o $t/$1 $t/$1.o 2> $t/$1.log
  warned $t/$1.log $2 $t/$1.o
  local f=$(nm $t/$1 | awk '$3 == "_f" { print $1 }' | sed 's/^0*//')
  dwarfdump --eh-frame $t/$1 | grep -q "FDE cie=.* pc=0*$f\.\.\."
  objdump --unwind-info $t/$1 > $t/$1.unwind
  not grep -qi "function offset=0x0*${f: -5}," $t/$1.unwind
  # _main has no unwind info: an __unwind_info entry of encoding 0, or
  # no table at all.
  if otool -l $t/$1 | grep -q 'sectname __unwind_info'; then
    [ "$(unwind_lookup $t/$1 _main)" = 0x0 ]
  fi
  $mold -r -arch $ARCH -o $t/$1-r.o $t/$1.o 2> $t/$1-r.log
  warned $t/$1-r.log $2 $t/$1.o
  dwarfdump --eh-frame $t/$1-r.o | grep -q 'FDE cie='
}

obj bar '.section __DATA,__bar'
carried bar __DATA,__bar

if $mold -v 2>&1 | grep -q mold-macho; then
  obj data .data
  carried data __DATA,__data
  obj const '.section __TEXT,__const'
  carried const __TEXT,__const
  obj cstring '.section __TEXT,__cstring,cstring_literals'
  carried cstring __TEXT,__cstring

  # A section with instructions is code, whatever its name. (The
  # assembler drops the attribute from __DATA,__data, a name it knows.)
  obj pure '.section __DATA,__xcode,regular,pure_instructions'
  $CC --ld-path=$mold -o $t/pure $t/pure.o 2> $t/pure.log
  not grep -q 'unwind information' $t/pure.log
  f=$(nm $t/pure | awk '$3 == "_f" { print $1 }' | sed 's/^0*//')
  objdump --unwind-info $t/pure | grep -qi "function offset=0x0*${f: -5},"
fi

# A section with no code, the functions of which have compact unwind:
# the same warning, once. (clang gives an x86_64 simulator's functions
# outside code none.)
on_simulator && [ $ARCH = x86_64 ] && exit 0
cat <<EOF | $CC -o $t/compact.o -c -xassembler -
.section __DATA,__bar
.globl _g
.p2align 2
_g:
  .cfi_startproc
  .long 0
  .cfi_endproc
.globl _h
.p2align 2
_h:
  .cfi_startproc
  .long 0
  .cfi_endproc
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/compact-r.o $t/compact.o 2> $t/compact.log
warned $t/compact.log __DATA,__bar $t/compact.o
[ "$(grep -c 'unwind information' $t/compact.log)" = 1 ]
