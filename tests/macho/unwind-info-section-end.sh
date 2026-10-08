#!/bin/bash
source "$(dirname "$0")"/common.inc

# A compact unwind record's function is looked for in the section its
# relocation names, as for a label: one at the section's end is the
# last subsection's, or that of a label there, and not the next
# section's, which keeps its own.
if [ $ARCH = arm64 ]; then ret=ret; else ret=retq; fi
rec() { printf '.quad %s\n.long 0\n.long %s\n.quad 0\n.quad 0\n' $1 $2; }

for sub in '' .subsections_via_symbols; do
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  $ret
_f:
  $ret
Lend:
.section __LD,__compact_unwind,regular,debug
.p2align 3
$(rec Lend 0x02001000)
$sub
EOF
  $CC --ld-path=$mold -o $t/exe $t/a.o
  [ "$(unwind_lookup $t/exe _main _f | tr '\n' ' ')" = '0x0 0x0 ' ]

  # A label at the end takes the record.
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  $ret
_f:
  $ret
_end:
.section __TEXT,__text2,regular,pure_instructions
_x:
  $ret
.section __LD,__compact_unwind,regular,debug
.p2align 3
$(rec _end 0x02001000)
$(rec _x 0x02002000)
$sub
EOF
  $CC --ld-path=$mold -o $t/exe2 $t/b.o
  [ "$(unwind_lookup $t/exe2 _main _f _x | tr '\n' ' ')" = '0x0 0x0 0x2002000 ' ]
done
