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
