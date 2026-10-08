#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ "$ARCH" = arm64 ] || skip

# A b/bl reaches 128 MiB and an adrp 4 GiB. A branch to code in a
# segment -segaddr puts farther than 128 MiB away goes through a range
# extension thunk, whose adrp reaches 4 GiB, and a GOT load of it
# relaxes to an adrp+add; a reference 4 GiB away or more is an error, as
# an out-of-range reference is in mold, the actual distance deciding.
# (ld-prime decides before layout which segments are 4 GiB apart, taking
# an unpinned segment to be a page past the image's base, and calls code
# between them through a stub that jumps through a rebased __got slot,
# which a GOT load of the code shares, unrelaxed.)
mold_only() { $mold -v 2>&1 | grep -q mold-macho; }

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

# 256 MiB away: thunks. (ld-prime makes no branch island in __TEXT for
# a branch to another segment, and fails with a fixup error.)
if mold_only; then
  $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-segaddr,__FAR,0x110000000 \
    -Wl,-segprot,__FAR,rx,rx
  $RUN $t/exe || [ $? = 82 ]
  otool -tv $t/exe | grep -A8 '^_main:' > $t/main
  grep -Eq 'add[[:space:]]+x8, x8' $t/main
fi

# 4 GiB from __TEXT: out of reach.
not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-segaddr,__FAR,0x200000000 2> $t/log2
grep -q 'B/BL out of range' $t/log2

# 8 GiB away, where ld-prime's stubs reach, and in a -static image,
# which has no dyld to rebase a stub's slot.
if mold_only; then
  not $CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-segaddr,__FAR,0x300000000 \
    -Wl,-segprot,__FAR,rx,rx 2> $t/log3
  grep -q 'B/BL out of range' $t/log3
  grep -q 'ADRP out of range' $t/log3
fi
not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segaddr,__FAR,0x300000000 \
  -static -nostdlib -Wl,-e,_main 2> $t/log4
grep -q 'B/BL out of range' $t/log4

# In a dylib with __TEXT pinned 8 GiB up, an unpinned segment lands right
# after it, within reach.
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
otool -Iv $t/c.dylib > $t/indirect
if mold_only; then
  grep -Eq 'add[[:space:]]+x0, x0' $t/f
  grep -Eq 'b[[:space:]]+_g' $t/f
  not grep -q '__TEXT,__stubs' $t/indirect
fi
