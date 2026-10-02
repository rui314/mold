#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output's contents mirror its address space: a zero-fill section
# takes no file space and comes after the others, and each other one
# lies at its address's distance from the first section, so that it is
# aligned in the file as in memory.

# Links $1 bytes of data, a 16-aligned __bss, $2 bytes in __q and an
# 8-aligned __r, and prints how far the file offsets of __q and __r
# from __data's differ from their addresses', __r's offset modulo 8,
# the last section and its file offset: "0 0 0 __bss 0" if all is well.
check() {
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
  otool -l $t/$1-$2.r.o > $t/$1-$2.lc
  field() { awk -v s=$1 -v f=$2 '$1 == "sectname" { n = $2 } n == s && $1 == f { print $2; exit }' $t/$3.lc; }
  local d=$(field __data offset $1-$2) q=$(field __q offset $1-$2) r=$(field __r offset $1-$2)
  local da=$(field __data addr $1-$2) qa=$(field __q addr $1-$2) ra=$(field __r addr $1-$2)
  local last=$(grep sectname $t/$1-$2.lc | tail -1 | awk '{print $2}')
  echo $((q - d - (qa - da))) $((r - d - (ra - da))) $((r % 8)) $last $(field $last offset $1-$2)
}

[ "$(check 1 4)" = '0 0 0 __bss 0' ]
[ "$(check 4 4)" = '0 0 0 __bss 0' ]
[ "$(check 3 5)" = '0 0 0 __bss 0' ]
[ "$(check 4 2)" = '0 0 0 __bss 0' ]
