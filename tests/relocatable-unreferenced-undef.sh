#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime keeps an undefined symbol in a -r output only if a
# relocation of the output refers to it - an unwind personality's
# included - or the command line makes it an initial undefine (-u): a
# stray .globl, or a weak, lazy or no-dead-strip reference nothing
# uses, goes.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _g2
.weak_reference _g4
.reference _g6
.lazy_reference _g7
.text
.globl _f
_f:
  ret
.data
_d:
  .quad _g3
EOF
$mold -r -arch $ARCH -o $t/r.o $t/a.o -u _g8
nm -m $t/r.o > $t/log
grep -q '(undefined) external _g3$' $t/log
grep -q '(undefined) external _g8$' $t/log
not grep -q '_g[2467]$' $t/log

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
int f();
int g() { try { return f(); } catch (...) { return 0; } }
EOF
$mold -r -arch $ARCH -o $t/r2.o $t/b.o
nm -m $t/r2.o | grep -q '(undefined) external ___gxx_personality_v0$'
