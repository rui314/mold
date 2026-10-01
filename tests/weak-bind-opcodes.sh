#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime encodes the classic weak-bind stream as the bind stream: each
# piece of state is set only when it changes - the type once, the
# address moved by ADD_ADDR_ULEB within a segment, backwards too - and
# a bind followed by a step becomes DO_BIND_ADD_ADDR_IMM_SCALED, a run
# of equal steps DO_BIND_ULEB_TIMES_SKIPPING_ULEB.
cat <<EOF | $CC -o $t/a.o -c -xassembler - -mmacos-version-min=11.0
.text
.globl _main
.p2align 2
_main:
  ret
.globl _wa
.weak_definition _wa
_wa:
  ret
.globl _wb
.weak_definition _wb
_wb:
  ret
.data
.p2align 3
  .quad _wa
  .quad _wa
  .quad _wa
  .quad _wb
  .quad 0
  .quad _wa
  .quad _wb
  .quad 0
  .quad 0
  .quad _wb
  .quad _wb
  .quad _wb
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -mmacos-version-min=11.0

off=$(otool -l $t/exe | awk '/weak_bind_off/ { print $2 }')
size=$(otool -l $t/exe | awk '/weak_bind_size/ { print $2 }')
od -An -tx1 -j $off -N $size $t/exe | tr -d ' \n' > $t/stream
# SET_SYMBOL _wa, SET_TYPE_IMM pointer, SET_SEGMENT_AND_OFFSET __DATA+0,
# DO_BIND x2, DO_BIND_ADD_ADDR_IMM_SCALED 2, DO_BIND; SET_SYMBOL _wb,
# ADD_ADDR_ULEB -0x18, DO_BIND_ULEB_TIMES_SKIPPING_ULEB 2 0x10, DO_BIND x3,
# DONE, padding.
expected=405f7761005172009090b290405f776200
expected=${expected}80e8ffffffffffffffff01c00210909090
expected=${expected}000000000000
[ "$(cat $t/stream)" = $expected ]
