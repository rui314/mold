#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under a CIE that declares an LSDA ('L'), GCC gives every FDE an LSDA
# pointer, zero for a function that has no LSDA, and ld-prime reads a
# zero pointer, or an FDE with no augmentation data, as none. A CIE's
# personality ('P') may come in any encoding ld-prime reads, the
# personality being the one the GOT-relative relocation names, if any.
# Written out here in GCC's shape, 8-byte pc-relative pointers, for a
# frame an exception unwinds through.
if [ $ARCH = arm64 ]; then
  ra=30 got='___gxx_personality_v0@GOT - .'
  code='stp x29, x30, [sp, -16]!
  mov x29, sp
  bl _thrower
  ldp x29, x30, [sp], 16
  ret'
  init='.byte 0x0c, 31, 0'
  cfa='.byte 0x40 + 8, 0x0c, 29, 16, 0x80 + 30, 1, 0x80 + 29, 2'
else
  ra=16 got='___gxx_personality_v0@GOTPCREL'
  code='pushq %rbp
  movq %rsp, %rbp
  callq _thrower
  popq %rbp
  retq'
  init='.byte 0x0c, 7, 8, 0x90, 1'
  cfa='.byte 0x40 + 4, 0x0c, 6, 16, 0x80 + 6, 2'
fi

# obj <name> <augmentation> <CIE's augmentation data> <FDE's>
obj() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl _through_asm
.p2align 2
_through_asm:
  $code
Lend:

.section __TEXT,__eh_frame,coalesced,no_toc+strip_static_syms+live_support
EH_frame0:
  .long Lcie_end - Lcie_start
Lcie_start:
  .long 0
  .byte 1
  .asciz "$2"
  .byte 1, 0x78, $ra
  .byte Lcie_aug_end - Lcie_aug
Lcie_aug:
  $3
Lcie_aug_end:
  $init
  .p2align 3
Lcie_end:
  .long Lfde_end - Lfde_start
Lfde_start:
  .long Lfde_start - EH_frame0
  .quad _through_asm - .
  .quad Lend - _through_asm
  .byte Lfde_aug_end - Lfde_aug
Lfde_aug:
  $4
Lfde_aug_end:
  $cfa
  .p2align 3
Lfde_end:
.subsections_via_symbols
EOF
}

cat <<EOF | $CXX -o $t/main.o -c -xc++ -
#include <cstdio>
extern "C" void thrower() { throw 40; }
extern "C" void through_asm();
int main() { try { through_asm(); } catch (int e) { printf("caught %d\n", e + 2); } }
EOF

# run <name>: the exception passes the frame after a final link, and
# after a -r one and a final link by either linker; the FDE has no LSDA.
run() {
  $CXX --ld-path=$mold -o $t/$1 $t/$1.o $t/main.o &&
    $t/$1 | grep -q 'caught 42' &&
    $mold -r -arch $ARCH -o $t/$1-r.o $t/$1.o &&
    $CXX --ld-path=$mold -o $t/$1-r $t/$1-r.o $t/main.o &&
    $t/$1-r | grep -q 'caught 42' &&
    $CXX -o $t/$1-r2 $t/$1-r.o $t/main.o &&
    $t/$1-r2 | grep -q 'caught 42' &&
    objdump --unwind-info $t/$1 | sed -n '/LSDA descriptors:/,/Second level/p' > $t/$1.lsda &&
    f=$(nm $t/$1 | awk '$3 == "_through_asm" { print $1 }') &&
    not grep -q "function offset=0x${f: -8}," $t/$1.lsda
}

personality=".byte 0x9b
  .long $got"

# GCC's: a personality and a zero LSDA pointer.
obj gcc zPLR "$personality
  .byte 0x10, 0x10" '.quad 0'
run gcc

# No augmentation data, with an LSDA encoding that omits the pointer
# (0xff), whatever else ld-prime reads.
obj omit zLR '.byte 0xff, 0x10' ''
run omit

# An absolute (0x00) personality pointer with no relocation, none.
obj absptr zPR '.byte 0x00
  .quad 0
  .byte 0x10' ''
run absptr
