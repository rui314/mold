#!/bin/bash
. $(dirname $0)/common.inc

seq 1 100000 | sed 's/.*/int x\0 __attribute__((section(".data.\0")));/g' | \
  $CC -c -xc -o $t/a.o -

./mold --relocatable -o $t/b.o $t/a.o
readelf -WS $t/b.o | grep -Fq .data.100000
readelf -Ws $t/b.o | grep -Fq 'OBJECT  GLOBAL DEFAULT 100000'
