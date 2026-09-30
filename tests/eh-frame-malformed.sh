#!/bin/bash
source "$(dirname "$0")"/common.inc

# A malformed __eh_frame is an error in ld-prime's words, not a crash.
# ld-prime walks the records (a length, then an ID that is zero for a
# CIE and for an FDE the distance back to its CIE) and takes whatever
# an FDE points at for its CIE.

# fail <name> <message> <__eh_frame contents...>: the link fails with
# <message> about the object, or with some error about it if <message>
# is empty.
fail() {
  name=$1 msg=$2
  shift 2
  { printf '.text\n.globl _main\n.p2align 2\n_main: ret\n'
    printf '.section __TEXT,__eh_frame\n'
    printf '%s\n' "$@"; } | $CC -o $t/$name.o -c -xassembler -
  not $mold -r -arch $ARCH -o $t/$name-r.o $t/$name.o 2> $t/$name.err
  if [ -n "$msg" ]; then
    grep -Fq "$msg in '$t/$name.o'" $t/$name.err
  else
    grep -Fq "$t/$name.o" $t/$name.err
  fi
}

# A CIE with no augmentation, 16 bytes.
cie='.long 12
.long 0
.byte 1, 0, 1, 0x78, 30, 0, 0, 0'

# A record running past the section, by its length or a 64-bit extended
# one (not supported); the offset is from the start of the section.
fail long 'CFI at 0x00000000 extends beyond end of section' '.long 0x1000' '.long 0'
fail extended 'CFI at 0x00000000 extends beyond end of section' \
  '.long 0xffffffff' '.quad 8' '.quad 0'
fail second 'CFI at 0x00000010 extends beyond end of section' "$cie" '.long 20' '.long 0'

# An FDE whose CIE would be before the section, the FDE itself, or the
# CIE's zero ID.
fail outside 'FDE points to CIE outside __eh_frame section' \
  "$cie" '.long 20' '.long 0x100' '.quad 0' '.quad 0'
fail self 'CIE ID is not zero' "$cie" '.long 20' '.long 4' '.quad 0' '.quad 0'
fail id 'empty CIE' "$cie" '.long 20' '.long 16' '.quad 0' '.quad 0'

# A zero length, and a CIE version other than 1 and 3.
fail zero 'empty CIE' "$cie" '.long 0' '.long 0'
fail version 'CIE version is not 1 or 3' '.long 12' '.long 0' '.byte 2, 0, 1, 0x78, 30, 0, 0, 0'

# Records too short for their fields: a section too short for a length,
# a CIE with only its ID and an FDE with no function size. ld-prime
# reads the fields from past them, so only the error is certain.
fail short '' '.byte 1'
fail cie '' '.long 4' '.long 0'
fail fde '' "$cie" '.long 12' '.long 20' '.quad 0'

# cie_r <encoding>: a CIE whose 'R' augmentation gives how its FDEs
# encode their function, 24 bytes, labeled so that an FDE's pointer to
# _main (at address 0) is a SUBTRACTOR pair against the label.
cie_r() {
  printf '%s\n' 'EH_frame0:' '.long 20' '.long 0' \
    ".byte 1, 0x7a, 0x52, 0, 1, 0x78, 30, 1, $1, 0x0c, 31, 8, 0, 0, 0, 0"
}

# An FDE whose function is outside every section, at the address
# ld-prime reads for it: the pc-relative pointer's own plus its value.
fail nowhere 'not in any section' "$(cie_r 0x10)" '.long 20' '.long 28' '.quad 0x1000' '.quad 0'
eh=$(otool -l $t/nowhere.o | awk '$2 == "__eh_frame" { f = 1 } f && $1 == "addr" { print $2; exit }')
grep -Fq "address=0x$(printf %X $((eh + 0x1020))) not in any section" $t/nowhere.err

# ld-prime reads an FDE's function in a format of 4 or 8 bytes
# (DW_EH_PE_absptr 0x0, sdata4 0xb or sdata8 0xc), absolute or
# pc-relative (0x10), and with the top bit (DW_EH_PE_indirect) set or
# not; it refuses any other encoding as it reads the pointer. It looks
# the function up, then refuses all but 0x10 and 0x1b, clang's and
# GCC's. A CIE without an 'R' augmentation counts as 0x00.
fail enc03 'unsupported pointer encoding 0x03' "$(cie_r 0x03)" \
  '.long 12' '.long 28' '.long 0' '.long 0'
fail enc30 'unsupported pointer encoding 0x30' "$(cie_r 0x30)" \
  '.long 20' '.long 28' '.quad 0' '.quad 0'
fail encff 'unsupported pointer encoding 0xFF' "$(cie_r 0xff)" \
  '.long 20' '.long 28' '.quad 0' '.quad 0'
fail enc0c 'unsupported FDE pointer encoding 0x0C in FDE' "$(cie_r 0x0c)" \
  '.long 20' '.long 28' '.quad 0' '.quad 0'
fail enc1c 'unsupported FDE pointer encoding 0x1C in FDE' "$(cie_r 0x1c)" \
  '.long 20' '.long 28' '.quad _main - .' '.quad 0'
fail enc90 'unsupported FDE pointer encoding 0x90 in FDE' "$(cie_r 0x90)" \
  '.long 20' '.long 28' '.quad 0' '.quad 0'
fail noR 'unsupported FDE pointer encoding 0x00 in FDE' \
  'EH_frame0:' '.long 20' '.long 0' '.byte 1, 0x7a, 0, 1, 0x78, 30, 0, 0x0c, 31, 8, 0, 0, 0, 0, 0, 0' \
  '.long 20' '.long 28' '.quad 0' '.quad 0'
fail noz 'unsupported FDE pointer encoding 0x00 in FDE' "$cie" '.long 20' '.long 20' '.quad 0' '.quad 0'
