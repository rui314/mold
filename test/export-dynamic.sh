#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl foo
  .hidden foo
foo:
  nop
  .globl bar
bar:
  nop
  .globl _start
_start:
  nop
EOF

cc -shared -fPIC -o $t/b.so -xc /dev/null
../mold -o $t/exe $t/a.o $t/b.so --export-dynamic

readelf --dyn-syms $t/exe | grep -q "
Symbol table '.dynsym' contains 19 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
     1: 0000000000201010     0 NOTYPE  GLOBAL DEFAULT    6 foo
     2: 0000000000201011     0 NOTYPE  GLOBAL DEFAULT    6 bar
     3: 0000000000201012     0 NOTYPE  GLOBAL DEFAULT    6 _start
     4: 0000000000200000     0 NOTYPE  GLOBAL DEFAULT  ABS __ehdr_start
     5: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __rela_iplt_start
     6: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __rela_iplt_end
     7: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __init_array_start
     8: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __init_array_end
     9: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __fini_array_start
    10: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __fini_array_end
    11: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __preinit_array_start
    12: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __preinit_array_end
    13: 0000000000202018     0 NOTYPE  GLOBAL DEFAULT  ABS _DYNAMIC
    14: 0000000000202000     0 NOTYPE  GLOBAL DEFAULT  ABS _GLOBAL_OFFSET_TABLE_
    15: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __bss_start
    16: 0000000000202178     0 NOTYPE  GLOBAL DEFAULT  ABS _end
    17: 0000000000201013     0 NOTYPE  GLOBAL DEFAULT  ABS _etext
    18: 0000000000202178     0 NOTYPE  GLOBAL DEFAULT  ABS _edata
"

echo ' OK'
