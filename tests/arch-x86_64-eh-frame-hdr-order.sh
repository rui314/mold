#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Each function below is described by two FDEs, which have the same
# function address in the .eh_frame_hdr table. They must be ordered by
# their own addresses, so that the output is the same on every host. The
# FDEs are in the reverse order of the functions, so that sorting the
# table reorders the entries.
{
  echo '.text'
  for i in $(seq 1 100); do echo "f$i: .skip 16"; done
  echo '.section .eh_frame,"a",@progbits'
  echo 'cie: .long 16, 0; .byte 1; .asciz "zR"; .byte 1, 0x78, 16, 1, 0x1b, 0, 0, 0'
  for i in $(seq 100 -1 1); do
    echo ".long 16, . - cie, f$i - ., 16, 0"
    echo ".long 16, . - cie, f$i - ., 16, 0"
  done
} | $CC -c -xassembler -o $t/a.o -

$CC -B. -shared -o $t/b.so $t/a.o
$OBJCOPY -O binary --only-section=.eh_frame_hdr $t/b.so $t/hdr

# Skip the 12-byte header and check that the (function, FDE) pairs are sorted.
od -An -td4 -w8 -j12 -v $t/hdr > $t/log
[ $(wc -l < $t/log) = 200 ]
sort -c -k1,1n -k2,2n $t/log
