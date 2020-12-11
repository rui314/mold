#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .globl foo, bar, this_is_global
local1:
foo:
bar:
  .byte 0
EOF

cat <<EOF | cc -o $t/b.o -c -x assembler -
  .globl this_is_global
local2:
this_is_global:
EOF

../mold -o $t/exe $t/a.o $t/b.o > /dev/null

readelf --symbols $t/exe | grep -q "
Symbol table '.symtab' contains 21 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
     1: 0000000000201010     0 NOTYPE  LOCAL  DEFAULT    6 local1
     2: 0000000000201011     0 NOTYPE  LOCAL  DEFAULT    6 local2
     3: 0000000000201010     0 NOTYPE  GLOBAL DEFAULT    6 foo
     4: 0000000000201010     0 NOTYPE  GLOBAL DEFAULT    6 bar
     5: 0000000000201011     0 NOTYPE  GLOBAL DEFAULT    6 this_is_global
     6: 0000000000200000     0 NOTYPE  GLOBAL HIDDEN     1 __ehdr_start
     7: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __rela_iplt_start
     8: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __rela_iplt_end
     9: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __init_array_start
    10: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __init_array_end
    11: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __fini_array_start
    12: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __fini_array_end
    13: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __preinit_array_start
    14: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __preinit_array_end
    15: 0000000000202018     0 NOTYPE  GLOBAL HIDDEN     8 _DYNAMIC
    16: 0000000000202000     0 NOTYPE  GLOBAL HIDDEN     7 _GLOBAL_OFFSET_TABLE_
    17: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __bss_start
    18: 0000000000202168     0 NOTYPE  GLOBAL HIDDEN     8 _end
    19: 0000000000201011     0 NOTYPE  GLOBAL HIDDEN     6 _etext
    20: 0000000000202168     0 NOTYPE  GLOBAL HIDDEN     8 _edata
"

echo ' OK'
