// RUN: cc -o %t1.o -c %s
// RUN: echo '.globl this_is_global; local2: this_is_global:' | \
// RUN:   cc -x assembler -o %t2.o -c -
// RUN: mold -o %t.exe %t1.o %t2.o
// RUN: readelf --symbols %t.exe | FileCheck %s

// CHECK: Symbol table '.symtab' contains 22 entries:
// CHECK: Num:    Value          Size Type    Bind   Vis      Ndx Name
// CHECK:   0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
// CHECK:   1: 0000000000201000     0 NOTYPE  LOCAL  DEFAULT    8 local1
// CHECK:   2: 0000000000201001     0 NOTYPE  LOCAL  DEFAULT    8 local2
// CHECK:   3: fffffffffffffe70     0 NOTYPE  LOCAL  DEFAULT    1 __ehdr_start
// CHECK:   4: 00000000002001d0     0 NOTYPE  LOCAL  DEFAULT    5 __rela_iplt_start
// CHECK:   5: 00000000002001d0     0 NOTYPE  LOCAL  DEFAULT    5 __rela_iplt_end
// CHECK:   6: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __init_array_start
// CHECK:   7: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __init_array_end
// CHECK:   8: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __fini_array_start
// CHECK:   9: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __fini_array_end
// CHECK:  10: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __preinit_array_start
// CHECK:  11: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  ABS __preinit_array_end
// CHECK:  12: 0000000000201000     0 NOTYPE  GLOBAL DEFAULT    8 foo
// CHECK:  13: 0000000000201000     0 NOTYPE  GLOBAL DEFAULT    8 bar
// CHECK:  14: 0000000000201001     0 NOTYPE  GLOBAL DEFAULT    8 this_is_global
// CHECK:  15: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __bss_start
// CHECK:  16: 0000000000202110     0 NOTYPE  GLOBAL DEFAULT   11 _end
// CHECK:  17: 0000000000201001     0 NOTYPE  GLOBAL DEFAULT    8 _etext
// CHECK:  18: 0000000000202110     0 NOTYPE  GLOBAL DEFAULT   11 _edata
// CHECK:  19: 0000000000202110     0 NOTYPE  GLOBAL DEFAULT   11 end
// CHECK:  20: 0000000000201001     0 NOTYPE  GLOBAL DEFAULT    8 etext
// CHECK:  21: 0000000000202110     0 NOTYPE  GLOBAL DEFAULT   11 edata

        .globl foo, bar, this_is_global
local1:
foo:
bar:
        .byte 0
