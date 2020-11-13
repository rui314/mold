// RUN: cc -o %t1.o -c %s
// RUN: echo '.globl this_is_global; local2: this_is_global:' | \
// RUN:   cc -x assembler -o %t2.o -c -
// RUN: mold -o %t.exe %t1.o %t2.o
// RUN: readelf --symbols %t.exe | FileCheck %s

// CHECK: Symbol table '.symtab' contains 20 entries:
// CHECK: Num:    Value          Size Type    Bind   Vis      Ndx Name
// CHECK:   0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
// CHECK:   1: 0000000000201000     0 NOTYPE  LOCAL  DEFAULT    7 local1
// CHECK:   2: 0000000000201001     0 NOTYPE  LOCAL  DEFAULT    7 local2
// CHECK:   3: 0000000000200000     0 NOTYPE  LOCAL  DEFAULT    1 __ehdr_start
// CHECK:   4: 00000000002001b0     0 NOTYPE  LOCAL  DEFAULT    2 __rela_iplt_start
// CHECK:   5: 00000000002001b0     0 NOTYPE  LOCAL  DEFAULT    2 __rela_iplt_end
// CHECK:   6: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __init_array_start
// CHECK:   7: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __init_array_end
// CHECK:   8: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __fini_array_start
// CHECK:   9: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __fini_array_end
// CHECK:  10: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __preinit_array_start
// CHECK:  11: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __preinit_array_end
// CHECK:  12: 0000000000202018     0 NOTYPE  LOCAL  HIDDEN    11 _DYNAMIC
// CHECK:  13: 0000000000201000     0 NOTYPE  GLOBAL DEFAULT    7 foo
// CHECK:  14: 0000000000201000     0 NOTYPE  GLOBAL DEFAULT    7 bar
// CHECK:  15: 0000000000201001     0 NOTYPE  GLOBAL DEFAULT    7 this_is_global
// CHECK:  16: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __bss_start
// CHECK:  17: 0000000000202128     0 NOTYPE  GLOBAL DEFAULT   11 _end
// CHECK:  18: 0000000000201018     0 NOTYPE  GLOBAL DEFAULT    8 _etext
// CHECK:  19: 0000000000202128     0 NOTYPE  GLOBAL DEFAULT   11 _edata

        .globl foo, bar, this_is_global
local1:
foo:
bar:
        .byte 0
