#!/bin/bash
source "$(dirname "$0")"/common.inc

# A pc-relative reference that can't reach its target is a fixup error,
# not a wrapped-around displacement: an arm64 ADRP reaches 4 GiB of
# pages either way and a B/BL 128 MiB, an x86-64 rip-relative field
# 2 GiB. ld-prime names the fixup's kind after what it makes of the
# instructions, the atom and offset of the field, and where the
# reference goes from and to, naming the target.
fixup() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl start
.p2align 2
start:
$2
  ret
.data
.globl _x, _y
.p2align 3
_x: .quad 1
_y: .quad 2
.section __FAR,__text,regular,pure_instructions
.globl _far
_far: ret
.subsections_via_symbols
EOF
  not $mold -arch $ARCH -platform_version macos 14.0 14.0 -static -e start \
    -segaddr __DATA 0x200000000 -segaddr __FAR 0x300000000 $t/$1.o -o $t/$1 2> $t/$1.log
}
hex='0x[0-9A-F]{9}'

if [ $ARCH = arm64 ]; then
  fixup adrp '  adrp x0, _x@PAGE
  add x0, x0, _x@PAGEOFF'
  grep -Eq "fixup error \(kind=arm64_adrp_lo12\) at 'start' from adrp.o, ADRP out of range, from $hex to 0x200000000 \('_x'\)" $t/adrp.log

  fixup adrp2 '  adrp x0, _y@PAGE+16'
  grep -Eq "fixup error \(kind=arm64_adrp_addend\) at 'start' from adrp2.o, ADRP out of range, from $hex to 0x200000018 \('_y'\)" $t/adrp2.log

  fixup bl '  bl _far'
  grep -Eq "fixup error \(kind=arm64_b26\) at 'start' from bl.o, B/BL out of range \(displacement=[0-9]+, max is \+/-128MB\), from $hex to 0x300000000 \('_far'\)" $t/bl.log
else
  fixup rip '  nop
  movl _x(%rip), %eax'
  grep -Eq "fixup error \(kind=x86_64_rip\) at 'start'\+0x3 from rip.o, 32-bit RIP-relative reference out of range \(displacement=[0-9]+, max is \+/-2GB\), from $hex to 0x200000000 \('_x'\)" $t/rip.log

  fixup rip1 '  movb $2, _y(%rip)'
  grep -Eq "fixup error \(kind=x86_64_rip1\) at 'start'\+0x2 from rip1.o, .* to 0x200000008 \('_y'\)" $t/rip1.log

  fixup call '  call _far'
  grep -Eq "fixup error \(kind=x86_64_call\) at 'start'\+0x1 from call.o, .* to 0x300000000 \('_far'\)" $t/call.log

  # Reaching back is a negative displacement.
  cat <<EOF | $CC -o $t/back.o -c -xassembler -
.text
.globl start
start:
  movl _x(%rip), %eax
  ret
.data
.globl _x
_x: .long 1
EOF
  not $mold -arch $ARCH -platform_version macos 14.0 14.0 -static -e start \
    -segaddr __TEXT 0x300000000 -segaddr __DATA 0x200000000 $t/back.o -o $t/back 2> $t/back.log
  grep -Eq "displacement=-[0-9]+, max is \+/-2GB\), from 0x300000[0-9A-F]{3} to 0x200000000 \('_x'\)" $t/back.log
fi

# A GOT slot, which a load of an import goes through, is named ''.
cat <<EOF | $CC -o $t/ext.o -c -xassembler -
.data
.globl _ext
_ext: .quad 1
EOF
sdk=$(xcrun --show-sdk-path)
$mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" -dylib \
  -o $t/libext.dylib $t/ext.o -lSystem
if [ $ARCH = arm64 ]; then
  load='  adrp x0, _ext@GOTPAGE
  ldr x0, [x0, _ext@GOTPAGEOFF]'
  kind=arm64_was_adrp_ldr_got_load_got
else
  load='  movq _ext@GOTPCREL(%rip), %rax'
  kind=x86_64_was_rip_got_load_load_got
fi
cat <<EOF | $CC -o $t/got.o -c -xassembler -
.text
.globl _f
_f:
$load
  ret
EOF
not $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" -dylib \
  -o $t/got.dylib $t/got.o $t/libext.dylib -lSystem \
  -segaddr __DATA_CONST 0x200000000 -segaddr __DATA 0x210000000 2> $t/got.log
grep -Eq "fixup error \(kind=$kind\) at '_f'(\+0x3)? from got.o, .* to 0x200000000 \(''\)" $t/got.log

# So is a stub's, which ld-prime reports as an atom of its own
# stubs-got-file, the first stub's only.
cat <<EOF | $CC -o $t/ext2.o -c -xassembler -
.text
.globl _extf, _extg
_extf: ret
_extg: ret
EOF
$mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" -dylib \
  -o $t/libext2.dylib $t/ext2.o -lSystem
cat <<EOF | $CC -o $t/call.o -c -xc -
void extf(void), extg(void);
void f(void) { extf(); extg(); }
EOF
not $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" -dylib \
  -o $t/call.dylib $t/call.o $t/libext2.dylib -lSystem -segaddr __DATA_CONST 0x200000000 \
  2> $t/call.log
[ "$(grep -c 'fixup error' $t/call.log)" = 1 ]
grep -Eq "fixup error \(kind=(arm64_adrp_lo12|x86_64_rip)\) at 'anon-2'(\+0x2)? from stubs-got-file, .* to 0x200000000 \(''\)" $t/call.log

# Of several fixup errors, ld-prime reports only the first in the
# output file - by segment and section in load command order, then by
# address - whatever their kinds or the order of the inputs and their
# relocations, and prints the layout once. __A lies below __TEXT but
# after it in the file.
if [ $ARCH = arm64 ]; then
  far='  bl _far'
  near='  adrp x0, _x@PAGE'
  far_kind=arm64_b26
  near_at="'_b'"
  far_at="'_c'"
else
  far='  call _far'
  near='  movl _x(%rip), %eax'
  far_kind=x86_64_call
  near_at="'_b'+0x2"
  far_at="'_c'+0x1"
fi
cat <<EOF | $CC -o $t/first1.o -c -xassembler -
.section __A,__text,regular,pure_instructions
.globl _a
.p2align 2
_a:
$far
EOF
cat <<EOF | $CC -o $t/first2.o -c -xassembler -
.text
.globl start, _b, _c
.p2align 2
start:
  ret
_b:
$near
$far
_c:
$far
.data
.globl _x
_x: .quad 1
.section __FAR,__text,regular,pure_instructions
.globl _far
_far: ret
.subsections_via_symbols
EOF
first() {
  not $mold -arch $ARCH -platform_version macos 14.0 14.0 -static -e start \
    -segaddr __A 0x100000000 -segaddr __TEXT 0x200000000 -segaddr __DATA 0x400000000 \
    -segaddr __FAR 0x600000000 $t/first1.o $t/first2.o -o $t/first "$@" 2> $t/first.log
  [ "$(grep -c 'fixup error' $t/first.log)" = 1 ]
  [ "$(grep -c '^final section layout:$' $t/first.log)" = 1 ]
}

first
grep -q "fixup error (kind=[a-z0-9_]*) at $near_at from first2.o, " $t/first.log

printf '_c\n_b\n' > $t/first.order
first -order_file $t/first.order
grep -q "fixup error (kind=$far_kind) at $far_at from first2.o, " $t/first.log
