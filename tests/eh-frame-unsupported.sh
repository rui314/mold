#!/bin/bash
source "$(dirname "$0")"/common.inc

# __eh_frame is a run of records, each a 4-byte length and a 4-byte ID
# that is zero for a CIE and, for an FDE, the distance back from the ID
# to its CIE. The linker reads what it rewrites - an FDE's CIE pointer,
# function and LSDA pointers, a CIE's personality - and fails the link,
# -r too, on what it can't rewrite: an FDE pointing at no CIE, a CIE
# version other than 1 or 3, a function or LSDA pointer other than the
# pc-relative ones compilers write (0x10 and 0x1b), or a personality
# other than a 4-byte pc-relative reference to its GOT slot (0x9b).

# fail <name> <__eh_frame contents...>
fail() {
  name=$1
  shift
  { printf '.text\n.globl _main\n.p2align 2\n_main: ret\n'
    printf '.section __TEXT,__eh_frame\n'
    printf '%s\n' "$@"; } | $CC -o $t/$name.o -c -xassembler - &&
    not $mold -r -arch $ARCH -o $t/$name-r.o $t/$name.o 2> /dev/null &&
    not $CC --ld-path=$mold -o $t/$name $t/$name.o 2> /dev/null
}

# A CIE with no augmentation, 16 bytes.
cie='.long 12
.long 0
.byte 1, 0, 1, 0x78, 30, 0, 0, 0'

fail outside "$cie" '.long 20' '.long 0x100' '.quad 0' '.quad 0'
fail version '.long 12' '.long 0' '.byte 2, 0, 1, 0x78, 30, 0, 0, 0'

# cie_r <encoding>: a CIE whose 'R' augmentation gives how its FDEs
# encode their function, 24 bytes, labeled so that an FDE's pointer to
# _main (at address 0) is a SUBTRACTOR pair against the label. A CIE
# without an 'R' augmentation counts as 0x00.
cie_r() {
  printf '%s\n' 'EH_frame0:' '.long 20' '.long 0' \
    ".byte 1, 0x7a, 0x52, 0, 1, 0x78, 30, 1, $1, 0x0c, 31, 8, 0, 0, 0, 0"
}
fail enc0c "$(cie_r 0x0c)" '.long 20' '.long 28' '.quad 0' '.quad 0'
fail enc1c "$(cie_r 0x1c)" '.long 20' '.long 28' '.quad _main - .' '.quad 0'
fail enc90 "$(cie_r 0x90)" '.long 20' '.long 28' '.quad 0' '.quad 0'
fail unsigned "$(cie_r 0x00)" '.long 20' '.long 28' '.quad _main' '.quad 1'
fail noz "$cie" '.long 20' '.long 20' '.quad 0' '.quad 0'

# An FDE's LSDA pointer, in the encoding of the CIE's 'L' augmentation,
# when the FDE's augmentation data holds one and it is not zero.
cie_l() {
  printf '%s\n' 'EH_frame0:' '.long 20' '.long 0' \
    ".byte 1, 0x7a, 0x4c, 0x52, 0, 1, 0x78, 30, 2, $1, 0x10, 0x0c, 31, 8, 0, 0"
}
fail lsda1c "$(cie_l 0x1c)" \
  '.long 29' '.long 28' '.quad _main - .' '.quad 1' '.byte 8' '.quad _main - .'

# A personality written as a pointer to the function itself.
fail pers10 'EH_frame0:' '.long 28' '.long 0' \
  '.byte 1, 0x7a, 0x50, 0x52, 0, 1, 0x78, 30, 10, 0x10' '.quad _main - .' \
  '.byte 0x10, 0x0c, 31, 8, 0, 0'

# The assembler writes a personality in other encodings as it is told,
# a reference to the GOT slot all the same, of 8 bytes or 4.
for enc in 0x10 0x9c 0x8b; do
  cat <<EOF | $CC -o $t/pers$enc.o -c -xassembler -
.text
.globl _main, _pers
_main:
  .cfi_startproc
  .cfi_personality $enc, _pers
  .cfi_def_cfa_offset 16
  ret
  .cfi_endproc
_pers:
  ret
EOF
  not $CC --ld-path=$mold -o $t/exe $t/pers$enc.o 2> /dev/null
  not $mold -r -arch $ARCH -o $t/r.o $t/pers$enc.o 2> /dev/null
done
