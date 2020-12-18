#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

echo '.globl main; main:' | cc -o $t/a.o -c -x assembler -

../mold -o $t/exe /usr/lib/x86_64-linux-gnu/crt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
  $t/a.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
  /usr/lib/x86_64-linux-gnu/crtn.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
  /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
  /lib/x86_64-linux-gnu/libc.so.6 \
  /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
  /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2

readelf --dynamic $t/exe | grep -q "
Dynamic section at offset 0x2048 contains 26 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
 0x0000000000000007 (RELA)               0x2001c8
 0x0000000000000008 (RELASZ)             24 (bytes)
 0x0000000000000009 (RELAENT)            24 (bytes)
 0x0000000000000017 (JMPREL)             0x2001b0
 0x0000000000000002 (PLTRELSZ)           24 (bytes)
 0x0000000000000003 (PLTGOT)             0x202028
 0x0000000000000014 (PLTREL)             RELA
 0x0000000000000006 (SYMTAB)             0x2001e0
 0x000000000000000b (SYMENT)             24 (bytes)
 0x0000000000000005 (STRTAB)             0x200228
 0x000000000000000a (STRSZ)              83 (bytes)
 0x0000000000000004 (HASH)               0x20027c
 0x0000000000000019 (INIT_ARRAY)         0x202210
 0x000000000000001b (INIT_ARRAYSZ)       8 (bytes)
 0x000000000000001a (FINI_ARRAY)         0x202208
 0x000000000000001c (FINI_ARRAYSZ)       8 (bytes)
 0x000000006ffffff0 (VERSYM)             0x20029c
 0x000000006ffffffe (VERNEED)            0x2002a8
 0x000000006fffffff (VERNEEDNUM)         1
 0x0000000000000015 (DEBUG)              0x0
 0x000000000000000c (INIT)               0x201030
 0x000000000000000d (FINI)               0x201020
 0x0000000000000000 (NULL)               0x0
"

readelf --symbols --use-dynamic $t/exe | grep -q "
Symbol table for image:
  Num Buc:    Value          Size   Type   Bind Vis      Ndx Name
    1   1: 0000000000000000   483 FUNC    GLOBAL DEFAULT UND __libc_start_main
    2   2: 0000000000000000   204 FUNC    GLOBAL DEFAULT UND printf
"

echo ' OK'
