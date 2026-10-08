#!/bin/bash
source "$(dirname "$0")"/common.inc

# The x86_64 assembler leaves a difference of two labels to the linker
# when a named label separates them, as the two may move apart, and
# writes a SUBTRACTOR/UNSIGNED pair for it. A label that no named
# label precedes in its section has no symbol to name, so its half of
# the pair is non-extern: it names the section, and the label's address
# goes into the contents. Here that is the CIE an FDE points back at,
# once a named label starts the FDE. The arm64 assembler resolves the
# difference itself.
#
# ld-prime, like ld64, takes the section ordinal of a non-extern
# subtrahend in __eh_frame for a symbol index (and asserts if there is
# no such symbol), which comes out right only if that symbol is at 0:
# here the section is 2 and symbol 2 is _main, after _main.eh and _f.
[ $ARCH = x86_64 ] || skip

# link <name> <CIE label>
link() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  push %rbp
  mov %rsp, %rbp
  call _f
  pop %rbp
  ret
Lmain_end:
.globl _f
_f:
  xor %eax, %eax
  ret
.section __TEXT,__eh_frame
$2:
  .long Lcie_end - Lcie_start
Lcie_start:
  .long 0
  .byte 1
  .asciz "zR"
  .byte 1, 0x78, 16
  .byte 1, 0x10
  .byte 0x0c, 7, 8, 0x90, 1
  .p2align 3
Lcie_end:
_main.eh:
  .long Lfde_end - Lfde_start
Lfde_start:
  .long Lfde_start - $2
  .quad _main - .
  .quad Lmain_end - _main
  .byte 0
  .byte 0x41, 0x0e, 0x10, 0x86, 0x02, 0x43, 0x0d, 0x06
  .p2align 3
Lfde_end:
.subsections_via_symbols
EOF
  $CC --ld-path=$mold -o $t/$1 $t/$1.o
  otool -s __TEXT __eh_frame $t/$1 | sed 1d > $t/$1.eh
  otool -s __TEXT __unwind_info $t/$1 | sed 1d > $t/$1.unwind

  $mold -r -arch $ARCH -o $t/$1-r.o $t/$1.o
  otool -s __TEXT __eh_frame $t/$1-r.o | sed 1d > $t/$1-r.eh
}

link named EH_frame0
link local Lcie

otool -rv $t/local.o > $t/local.rel
grep -Eq 'long +False +SUB' $t/local.rel

# Both give _main the same DWARF unwind info.
objdump --unwind-info $t/local > $t/local.info
grep -q 'encoding.*=0x04000' $t/local.info
cmp $t/named.eh $t/local.eh
cmp $t/named.unwind $t/local.unwind
cmp $t/named-r.eh $t/local-r.eh
