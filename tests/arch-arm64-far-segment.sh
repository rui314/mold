#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ "$ARCH" = arm64 ] || skip

# A segment -segaddr puts 4 GiB away is out of reach of a b/bl and of
# an adrp. ld-prime calls its code through a stub that jumps through a
# rebased __got slot, which a GOT load of the code shares, unrelaxed;
# it judges the distance by the pins, taking an unpinned segment to be
# a page past the image's base.
cat <<EOF > $t/a.s
.subsections_via_symbols
.text
.globl _main
.p2align 2
_main:
  stp x29, x30, [sp, #-16]!
  bl _far
  mov w19, w0
  bl _far_local
  add w19, w19, w0
  adrp x8, _far@GOTPAGE
  ldr x8, [x8, _far@GOTPAGEOFF]
  blr x8
  add w0, w19, w0
  ldp x29, x30, [sp], #16
  ret
.section __FAR,__text,regular,pure_instructions
.globl _far
.p2align 2
_far:
  mov w0, #40
  ret
_far_local:
  stp x29, x30, [sp, #-16]!
  bl _far
  sub w0, w0, #38
  ldp x29, x30, [sp], #16
  ret
EOF
$CC -o $t/a.o -c $t/a.s

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-segaddr,__FAR,0x300000000 \
  -Wl,-segprot,__FAR,rx,rx
$RUN $t/exe || [ $? = 82 ]
otool -tv $t/exe | grep -A8 '^_main:' > $t/main
grep -Eq 'ldr[[:space:]]+x8, \[x8\]' $t/main
otool -Iv $t/exe > $t/indirect
grep -q 'Indirect symbols for (__TEXT,__stubs) 2 entries' $t/indirect
grep -A3 '__TEXT,__stubs' $t/indirect | grep -q LOCAL

# 4 GiB from __TEXT, less than a page past its base: no stub, and so
# out of reach.
not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-segaddr,__FAR,0x200000000 2> $t/log2
grep -q 'B/BL out of range' $t/log2
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-segaddr,__FAR,0x200004000

# A -static image has no dyld to rebase a stub's slot.
not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segaddr,__FAR,0x300000000 \
  -static -nostdlib -Wl,-e,_main 2> $t/log4
grep -q 'B/BL out of range' $t/log4

# In a dylib with __TEXT pinned 8 GiB up, an unpinned segment is taken
# to be far from it, wherever it lands.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.subsections_via_symbols
.text
.globl _f
.p2align 2
_f:
  adrp x0, _d@GOTPAGE
  ldr x0, [x0, _d@GOTPAGEOFF]
  b _g
.section __TEXT2,__text,regular,pure_instructions
.globl _g
_g:
  ret
.data
.globl _d
_d:
  .quad 0
EOF
$CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -Wl,-segaddr,__TEXT,0x200000000
otool -tv $t/c.dylib | grep -A3 '^_f:' > $t/f
grep -Eq 'ldr[[:space:]]+x0, \[x0(, #0x[0-9a-f]+)?\]' $t/f
otool -Iv $t/c.dylib | grep -A2 '__TEXT,__stubs' | grep -q LOCAL
