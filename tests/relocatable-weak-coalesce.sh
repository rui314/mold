#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 -r keeps one copy of each weak definition (C++ inline
# functions and templates, Swift's per-object metadata records), the
# marker still on it so the final link can auto-hide it. We carried
# every copy: Xcode's -r prelink of NetNewsWire's RSCore package had a
# __DATA,__const twice the size of ld-prime's, and a __text 27KB
# larger. Unwind records of the dropped copies go with them.
cat <<EOF2 | $CC -O2 -o $t/a.o -c -xc++ -
template <typename T> struct Box { T v; __attribute__((noinline)) T get() const { return v; } };
inline int twice(int x) { return 2 * x; }
int use_a(Box<int> *b) { return b->get() + twice(1); }
EOF2
cat <<EOF2 | $CC -O2 -o $t/b.o -c -xc++ -
template <typename T> struct Box { T v; __attribute__((noinline)) T get() const { return v; } };
inline int twice(int x) { return 2 * x; }
int use_a(Box<int> *);
int main() { Box<int> b = {3}; return use_a(&b) + b.get() + twice(2) - 12; }
EOF2
$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
nm -m $t/r.o > $t/nm
[ "$(grep -c '__ZNK3BoxIiE3getEv$' $t/nm)" = 1 ]
grep -q 'weak external automatically hidden __ZNK3BoxIiE3getEv' $t/nm
# One __LD,__compact_unwind record per surviving function.
$CC --ld-path=$mold -o $t/exe $t/r.o
$RUN $t/exe

# Two copies of a weak definition may differ (Swift's __swift5_typeref
# strings come with or without a pad byte from one object to the next);
# ld64 discards the loser regardless, and so do we: the content is the
# compiler's promise, not compared.
cat <<EOF2 | $CC -o $t/w1.o -c -xassembler -
.section __TEXT,__swift5_typeref
.globl _sym
.weak_definition _sym
.private_extern _sym
_sym: .asciz "same-content"
.byte 0
.globl _other
.weak_definition _other
.private_extern _other
_other: .asciz "aaaa"
.text
.globl _w1
.p2align 2
_w1: ret
.subsections_via_symbols
EOF2
cat <<EOF2 | $CC -o $t/w2.o -c -xassembler -
.section __TEXT,__swift5_typeref
.globl _sym
.weak_definition _sym
.private_extern _sym
_sym: .asciz "same-content"
.globl _other
.weak_definition _other
.private_extern _other
_other: .asciz "bbbb"
.text
.globl _w2
.p2align 2
_w2: ret
.subsections_via_symbols
EOF2
$mold -r -arch $ARCH -o $t/w.o $t/w1.o $t/w2.o
otool -l $t/w.o | grep -A3 'sectname __swift5_typeref' | grep 'size 0x0000000000000013'

# So do copies of code of different sizes (an inline function compiled
# at different optimization levels): the kept copy, the first at equal
# alignment, stands for the others, and their bytes, relocations and
# unwind records go, in -r and in the final link alike. The output is
# then the size it would be without the dropped copy.
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
insn=$([ $ARCH = arm64 ] && echo bl || echo call)
cat <<EOF2 > $t/big.s
.text
.globl _sized
.weak_definition _sized
.p2align 2
_sized:
  .cfi_startproc
  nop
  $insn _callee
  nop
  ret
  .cfi_endproc
.subsections_via_symbols
EOF2
cat <<EOF2 > $t/small.s
.text
.globl _sized
.weak_definition _sized
.p2align 2
_sized:
  .cfi_startproc
  ret
  .cfi_endproc
.globl _other2
.p2align 2
_other2:
  ret
.subsections_via_symbols
EOF2
$CC -o $t/big.o -c $t/big.s
$CC -o $t/small.o -c $t/small.s
cat <<EOF2 | $CC -o $t/other.o -c -xassembler -
.text
.globl _other2
.p2align 2
_other2:
  ret
.subsections_via_symbols
EOF2
echo 'void callee(void) {}' | $CC -o $t/callee.o -c -xc -

sizes() { otool -l $1 | grep -A4 -e 'sectname __text' -e 'sectname __compact_unwind' | grep size; }
$mold -r -arch $ARCH -o $t/s1.o $t/big.o $t/small.o
$mold -r -arch $ARCH -o $t/s2.o $t/big.o $t/other.o
[ "$(sizes $t/s1.o)" = "$(sizes $t/s2.o)" ]
$mold -r -arch $ARCH -o $t/s3.o $t/small.o $t/big.o
$mold -r -arch $ARCH -o $t/s4.o $t/small.o
[ "$(sizes $t/s3.o)" = "$(sizes $t/s4.o)" ]
otool -rv $t/s3.o > $t/relocs3
not grep -q _callee $t/relocs3
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/callee.o $t/small.o $t/big.o
$CC --ld-path=$mold -o $t/exe4 $t/main.o $t/callee.o $t/small.o
[ "$(sizes $t/exe3)" = "$(sizes $t/exe4)" ]
$RUN $t/exe3

# Each copy of a weak function brings its own LC_DATA_IN_CODE entries,
# and only the kept copy's are written; a dropped copy's must not land
# a second time at the kept copy's address. The final link coalesces
# the same way.
for n in 1 2; do
  cat <<EOF2 | $CC -o $t/d$n.o -c -xassembler -
.text
.globl _jt
.weak_definition _jt
.p2align 2
_jt:
  ret
.data_region jt32
  .long $n
.end_data_region
.subsections_via_symbols
EOF2
done
$mold -r -arch $ARCH -o $t/d.o $t/d1.o $t/d2.o
otool -G $t/d.o > $t/dice
grep -q '(1 entries)' $t/dice
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/d1.o $t/d2.o -Wl,-u,_jt
otool -G $t/exe2 > $t/dice2
grep -q '(1 entries)' $t/dice2
