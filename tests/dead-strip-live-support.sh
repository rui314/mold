#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under -dead_strip, an atom of an S_ATTR_LIVE_SUPPORT section lives
# only if it references a live atom, and then keeps what it references.
# ld-prime checks them once, in input order, after what the roots reach
# is marked: _lvE, whose only target _lvF is made live by that check
# after _lvE's turn, stays dead, while _lvG, checked after _lvF, lives.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main: ret
_dead: ret
_kept: ret
.section __DATA,__live,regular,live_support
.p2align 3
_lvA: .quad _main
_lvB: .quad _dead
_lvC: .quad 3
_lvD: .quad _main
      .quad _kept
_lvE: .quad _lvF
_lvF: .quad _main
_lvG: .quad _lvF
_lvH: .quad _printf
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip
nm $t/exe | awk '{ print $NF }' | grep -v __mh_execute_header | sort | tr '\n' ' ' > $t/syms
[ "$(cat $t/syms)" = '_kept _lvA _lvD _lvF _lvG _main ' ]
