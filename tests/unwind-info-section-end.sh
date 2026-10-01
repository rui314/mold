#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime looks for a compact unwind record's function in the section
# its relocation names, as for a label: one at the section's end is the
# last subsection's (here _f's, which then gets no entry of encoding 0
# of its own), or that of a label there, and not the next section's.
if [ $ARCH = arm64 ]; then ret=ret; n=4; else ret=retq; n=1; fi
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
  objdump --unwind-info $t/exe > $t/unwind
  main=$(nm $t/exe | awk '$3 == "_main" { print $1 }')
  sed -En 's/.*function offset=0x([0-9a-f]+), encoding\[[0-9]+\]=(0x[0-9a-f]+)$/\1 \2/p' $t/unwind |
    while read off enc; do echo $(((0x$off + 0x100000000 - 0x$main) / n)) $enc; done > $t/entries
  printf '0 0x00000000\n2 0x02001000\n' | diff - $t/entries

  # A label at the end takes the record, and _f gets its entry.
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
  objdump --unwind-info $t/exe2 > $t/unwind2
  main=$(nm $t/exe2 | awk '$3 == "_main" { print $1 }')
  sed -En 's/.*function offset=0x([0-9a-f]+), encoding\[[0-9]+\]=(0x[0-9a-f]+)$/\1 \2/p' $t/unwind2 |
    while read off enc; do echo $(((0x$off + 0x100000000 - 0x$main) / n)) $enc; done > $t/entries2
  printf '0 0x00000000\n1 0x00000000\n2 0x02001000\n2 0x02002000\n' | diff - $t/entries2
done
