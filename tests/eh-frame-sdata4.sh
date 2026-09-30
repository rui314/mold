#!/bin/bash
source "$(dirname "$0")"/common.inc

# An FDE's function address and size, and its LSDA pointer, come in the
# encodings its CIE's 'R' and 'L' augmentations give. clang writes 8-byte
# pc-relative ones (DW_EH_PE_pcrel, 0x10); GCC 4-byte signed ones
# (DW_EH_PE_pcrel|DW_EH_PE_sdata4, 0x1b), which ld-prime reads and writes
# back in the same form, in a final link as in a -r one. Written out
# here, as no assembler directive picks the encoding, for a frame an
# exception unwinds through; its call site entry with no landing pad
# has the personality pass it on.
if [ $ARCH = arm64 ]; then
  ra=30 got='___gxx_personality_v0@GOT - .'
  code='stp x29, x30, [sp, -16]!
  mov x29, sp
Lcall:
  bl _thrower
Lret:
  ldp x29, x30, [sp], 16
  ret'
  # DW_CFA_def_cfa_offset sp, 0 in the CIE; past the prologue,
  # DW_CFA_def_cfa x29, 16 and x30 and x29 saved at CFA-8 and CFA-16.
  init='.byte 0x0c, 31, 0'
  cfa='.byte 0x40 + 8, 0x0c, 29, 16, 0x80 + 30, 1, 0x80 + 29, 2'
else
  ra=16 got='___gxx_personality_v0@GOTPCREL'
  code='pushq %rbp
  movq %rsp, %rbp
Lcall:
  callq _thrower
Lret:
  popq %rbp
  retq'
  # DW_CFA_def_cfa rsp, 8 and the return address at CFA-8 in the CIE;
  # past the prologue, DW_CFA_def_cfa rbp, 16 and rbp at CFA-16.
  init='.byte 0x0c, 7, 8, 0x90, 1'
  cfa='.byte 0x40 + 4, 0x0c, 6, 16, 0x80 + 6, 2'
fi

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _through_asm
.p2align 2
_through_asm:
  $code
Lend:

.section __TEXT,__gcc_except_tab
.p2align 2
Lexc:
  .byte 0xff, 0xff, 0x01
  .uleb128 Lcs_end - Lcs_start
Lcs_start:
  .uleb128 Lcall - _through_asm
  .uleb128 Lret - Lcall
  .uleb128 0
  .uleb128 0
Lcs_end:

.section __TEXT,__eh_frame,coalesced,no_toc+strip_static_syms+live_support
EH_frame0:
  .long Lcie_end - Lcie_start
Lcie_start:
  .long 0
  .byte 1
  .asciz "zPLR"
  .byte 1, 0x78, $ra
  .byte 7, 0x9b
  .long $got
  .byte 0x1b, 0x1b
  $init
  .p2align 2
Lcie_end:
  .long Lfde_end - Lfde_start
Lfde_start:
  .long Lfde_start - EH_frame0
  .long _through_asm - .
  .long Lend - _through_asm
  .byte 4
  .long Lexc - .
  $cfa
  .p2align 2
Lfde_end:
.subsections_via_symbols
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
#include <cstdio>
extern "C" void thrower() { throw 40; }
extern "C" void through_asm();
int main() { try { through_asm(); } catch (int e) { printf("caught %d\n", e + 2); } }
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'caught 42'

# The FDE keeps its 4-byte fields, which name the function and the LSDA
# where they are now.
sym() { nm $t/$1 | awk -v s=$2 '$3 == s { print $1 }' | sed 's/^0*//'; }
dwarfdump --eh-frame $t/exe > $t/eh
grep -q 'Augmentation data: *9B .. .. .. .. 1B 1B$' $t/eh
grep -q "FDE cie=00000000 pc=0*$(sym exe _through_asm)\.\.\." $t/eh
lsda=$(otool -l $t/exe | awk '$2 == "__gcc_except_tab" { f = 1 } f && $1 == "addr" { print $2; exit }')
grep -q "LSDA Address: 0*${lsda#0x}$" $t/eh

# So does a -r output, relocated by nothing but the personality's GOT
# reference, which either linker then links.
$mold -r -arch $ARCH -o $t/r.o $t/a.o
dwarfdump --eh-frame $t/r.o | grep -q 'Augmentation data: *9B .. .. .. .. 1B 1B$'
dwarfdump --eh-frame $t/r.o | grep -q 'FDE cie=00000000 pc=0*0\.\.\.'
otool -rv $t/r.o | sed -n '/(__TEXT,__eh_frame)/,/^Relocation information (__/p' > $t/r.relocs
[ "$(grep -c '___gxx_personality_v0$' $t/r.relocs)" = 1 ]
not grep -q 'SUB ' $t/r.relocs
$CXX --ld-path=$mold -o $t/exe2 $t/r.o $t/b.o
$t/exe2 | grep -q 'caught 42'
$CXX -o $t/exe3 $t/r.o $t/b.o
$t/exe3 | grep -q 'caught 42'

# For a shared cache builder that slides the sections apart
# (-add_split_seg_info), a 4-byte field is a 32-bit delta.
$CXX --ld-path=$mold -shared -o $t/c.dylib $t/a.o $t/b.o -Wl,-add_split_seg_info
dyld_info -shared_region $t/c.dylib > $t/split
grep -Eq '__eh_frame +0x[0-9a-f]+ +__TEXT +__text +0x[0-9a-f]+ +3$' $t/split
grep -Eq '__eh_frame +0x[0-9a-f]+ +__TEXT +__gcc_except_tab +0x[0-9a-f]+ +3$' $t/split
