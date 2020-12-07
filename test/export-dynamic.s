// RUN: cc -o %t1.o -c %s
// RUN: cc -shared -fPIC -o %t2.so -xc - < /dev/null
// RUN: mold -o %t.exe %t1.o %t2.so --export-dynamic

// RUN: readelf --dyn-syms %t.exe | FileCheck %s
// CHECK: Symbol table '.dynsym' contains 19 entries:
// CHECK:    Num:    Value          Size Type    Bind   Vis      Ndx Name
// CHECK:      0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
// CHECK:      1: 0000000000201010     0 NOTYPE  GLOBAL DEFAULT    6 foo
// CHECK:      2: 0000000000201011     0 NOTYPE  GLOBAL DEFAULT    6 bar
// CHECK:      3: 0000000000201012     0 NOTYPE  GLOBAL DEFAULT    6 _start
// CHECK:      4: 0000000000200000     0 NOTYPE  GLOBAL DEFAULT  ABS __ehdr_start
// CHECK:      5: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __rela_iplt_start
// CHECK:      6: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __rela_iplt_end
// CHECK:      7: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __init_array_start
// CHECK:      8: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __init_array_end
// CHECK:      9: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __fini_array_start
// CHECK:     10: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __fini_array_end
// CHECK:     11: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __preinit_array_start
// CHECK:     12: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __preinit_array_end
// CHECK:     13: 0000000000202018     0 NOTYPE  GLOBAL DEFAULT  ABS _DYNAMIC
// CHECK:     14: 0000000000202000     0 NOTYPE  GLOBAL DEFAULT  ABS _GLOBAL_OFFSET_TABLE_
// CHECK:     15: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  ABS __bss_start
// CHECK:     16: 0000000000202178     0 NOTYPE  GLOBAL DEFAULT  ABS _end
// CHECK:     17: 0000000000201013     0 NOTYPE  GLOBAL DEFAULT  ABS _etext
// CHECK:     18: 0000000000202178     0 NOTYPE  GLOBAL DEFAULT  ABS _edata

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
