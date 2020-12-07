// RUN: cc -o %t1.o -c %s
// RUN: echo '.globl this_is_global; local2: this_is_global:' | \
// RUN:   cc -x assembler -o %t2.o -c -
// RUN: mold -o %t.exe %t1.o %t2.o
// RUN: readelf --symbols %t.exe | FileCheck %s

// CHECK: Symbol table '.symtab' contains 21 entries:
// CHECK:    Num:    Value          Size Type    Bind   Vis      Ndx Name
// CHECK:      0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
// CHECK:      1: 0000000000201010     0 NOTYPE  LOCAL  DEFAULT    6 local1
// CHECK:      2: 0000000000201011     0 NOTYPE  LOCAL  DEFAULT    6 local2
// CHECK:      3: 0000000000201010     0 NOTYPE  GLOBAL DEFAULT    6 foo
// CHECK:      4: 0000000000201010     0 NOTYPE  GLOBAL DEFAULT    6 bar
// CHECK:      5: 0000000000201011     0 NOTYPE  GLOBAL DEFAULT    6 this_is_global
// CHECK:      6: 0000000000200000     0 NOTYPE  GLOBAL HIDDEN     1 __ehdr_start
// CHECK:      7: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __rela_iplt_start
// CHECK:      8: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __rela_iplt_end
// CHECK:      9: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __init_array_start
// CHECK:     10: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __init_array_end
// CHECK:     11: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __fini_array_start
// CHECK:     12: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __fini_array_end
// CHECK:     13: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __preinit_array_start
// CHECK:     14: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __preinit_array_end
// CHECK:     15: 0000000000202018     0 NOTYPE  GLOBAL HIDDEN     8 _DYNAMIC
// CHECK:     16: 0000000000202000     0 NOTYPE  GLOBAL HIDDEN     7 _GLOBAL_OFFSET_TABLE_
// CHECK:     17: 0000000000000000     0 NOTYPE  GLOBAL HIDDEN   ABS __bss_start
// CHECK:     18: 0000000000202168     0 NOTYPE  GLOBAL HIDDEN     8 _end
// CHECK:     19: 0000000000201011     0 NOTYPE  GLOBAL HIDDEN     6 _etext
// CHECK:     20: 0000000000202168     0 NOTYPE  GLOBAL HIDDEN     8 _edata

        .globl foo, bar, this_is_global
local1:
foo:
bar:
        .byte 0
