// RUN: cc -o %t.o -c %s
// RUN: mold -o %t.exe /usr/lib/x86_64-linux-gnu/crt1.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crti.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
// RUN:   /home/ruiu/mold/test/Output/hello-dynamic.s.tmp.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
// RUN:   /usr/lib/x86_64-linux-gnu/crtn.o \
// RUN:   /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
// RUN:   /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
// RUN:   /lib/x86_64-linux-gnu/libc.so.6 \
// RUN:   /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
// RUN:   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2

// RUN: readelf --dynamic %t.exe | FileCheck %s
// CHECK: Dynamic section at offset 0x2048 contains 23 entries:
// CHECK:   Tag        Type                         Name/Value
// CHECK:  0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
// CHECK:  0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
// CHECK:  0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
// CHECK:  0x0000000000000007 (RELA)               0x2001c8
// CHECK:  0x0000000000000008 (RELASZ)             24 (bytes)
// CHECK:  0x0000000000000009 (RELAENT)            24 (bytes)
// CHECK:  0x0000000000000017 (JMPREL)             0x2001b0
// CHECK:  0x0000000000000002 (PLTRELSZ)           24 (bytes)
// CHECK:  0x0000000000000003 (PLTGOT)             0x202028
// CHECK:  0x0000000000000014 (PLTREL)             RELA
// CHECK:  0x0000000000000006 (SYMTAB)             0x2001e0
// CHECK:  0x000000000000000b (SYMENT)             24 (bytes)
// CHECK:  0x0000000000000005 (STRTAB)             0x200228
// CHECK:  0x000000000000000a (STRSZ)              71 (bytes)
// CHECK:  0x0000000000000004 (HASH)               0x200270
// CHECK:  0x0000000000000019 (INIT_ARRAY)         0x2021e0
// CHECK:  0x000000000000001b (INIT_ARRAYSZ)       8 (bytes)
// CHECK:  0x000000000000001a (FINI_ARRAY)         0x2021d8
// CHECK:  0x000000000000001c (FINI_ARRAYSZ)       8 (bytes)
// CHECK:  0x0000000000000015 (DEBUG)              0x0
// CHECK:  0x000000000000000c (INIT)               0x201030
// CHECK:  0x000000000000000d (FINI)               0x201020
// CHECK:  0x0000000000000000 (NULL)               0x0

// RUN: readelf --symbols --use-dynamic %t.exe | FileCheck --check-prefix=DYNAMIC %s
	
// DYNAMIC: Symbol table for image:
// DYNAMIC:   Num Buc:    Value          Size   Type   Bind Vis      Ndx Name
// DYNAMIC:     1   1: 0000000000000000   483 FUNC    GLOBAL DEFAULT UND __libc_start_main
// DYNAMIC:     2   2: 0000000000000000   204 FUNC    GLOBAL DEFAULT UND printf

        .globl main
main:
