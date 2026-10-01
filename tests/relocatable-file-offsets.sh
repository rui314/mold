#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime packs a -r output's sections in the file: a zero-fill
# section takes no file space, and each other one follows the contents
# before it with the padding its address has - unless its file offset
# is aligned for it already, when it has none. Past a __bss, the next
# section starts where the contents before it ended, unaligned or not.

# Links $1 bytes of data, a 16-aligned __bss, $2 bytes in __q and an
# 8-aligned __r - __q at 0x1a0, past the __bss, and __r at 0x1a8 - and
# prints the file offsets of __q and __r from __data's.
offsets() {
  {
    echo '.section __DATA,__data'
    for i in $(seq $1); do echo '.byte 1'; done
    echo '.zerofill __DATA,__bss,_big,400,4'
    echo '.section __ZZZ,__q'
    for i in $(seq $2); do echo '.byte 1'; done
    echo '.section __ZZZ,__r'
    echo '.p2align 3'
    echo '.quad 1'
  } | $CC -o $t/$1-$2.o -c -xassembler -
  $mold -r -arch $ARCH $t/$1-$2.o -o $t/$1-$2.r.o
  otool -l $t/$1-$2.r.o |
    awk '$1 == "sectname" { s = $2 } $1 == "offset" { off[s] = $2 }
      END { print off["__q"] - off["__data"], off["__r"] - off["__data"] }'
}

# __q right after the data; __r 4 bytes later, as in the address space.
[ "$(offsets 1 4)" = "1 9" ]
# __r at the end of __q, 8-aligned in the file already.
[ "$(offsets 4 4)" = "4 8" ]
[ "$(offsets 3 5)" = "3 8" ]
# Not aligned: __r 6 bytes past __q, as the addresses are.
[ "$(offsets 4 2)" = "4 12" ]
