#!/bin/bash
source "$(dirname "$0")"/common.inc

# A pc-relative reference that can't reach its target is an error, not
# a wrapped-around displacement: an arm64 ADRP reaches 4 GiB of pages
# either way and a B/BL 128 MiB, an x86-64 rip-relative field 2 GiB.
# The error names the file, the subsection and offset of the field, and
# where the reference goes from and to, naming the target. (ld-prime
# words it otherwise, naming the fixup's kind.)
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
  grep -Eq "$t/adrp.o: start\+0x0: ADRP out of range, from $hex to 0x200000000 \('_x'\)" $t/adrp.log

  fixup adrp2 '  adrp x0, _y@PAGE+16'
  grep -Eq "$t/adrp2.o: start\+0x0: ADRP out of range, from $hex to 0x200000018 \('_y'\)" $t/adrp2.log

  # Back up to 4 GiB, which the immediate holds: from __TEXT at
  # 0x100000000 to the page of address 0. (ld-prime refuses that page.)
  for abs in 0xfff 0x1000; do
    cat <<EOF | $CC -o $t/abs.o -c -xassembler -
.globl _main
.p2align 2
_main:
  adrp x0, _abs@PAGE
  add x0, x0, _abs@PAGEOFF
  ret
.globl _abs
_abs = $abs
EOF
    $CC --ld-path=$mold -o $t/abs $t/abs.o
    rc=0
    $t/abs || rc=$?
    [ $rc = $((abs & 0xff)) ]
  done

  fixup bl '  bl _far'
  grep -Eq "$t/bl.o: start\+0x0: B/BL out of range \(displacement=[0-9]+, max is \+/-128MB\), from $hex to 0x300000000 \('_far'\)" $t/bl.log
else
  fixup rip '  nop
  movl _x(%rip), %eax'
  grep -Eq "$t/rip.o: start\+0x3: 32-bit RIP-relative reference out of range \(displacement=[0-9]+, max is \+/-2GB\), from $hex to 0x200000000 \('_x'\)" $t/rip.log

  fixup rip1 '  movb $2, _y(%rip)'
  grep -Eq "$t/rip1.o: start\+0x2: .* to 0x200000008 \('_y'\)" $t/rip1.log

  fixup call '  call _far'
  grep -Eq "$t/call.o: start\+0x1: .* to 0x300000000 \('_far'\)" $t/call.log

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

# A load of an import goes through its GOT slot, named after it.
# (ld-prime names a GOT slot ''.)
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
else
  load='  movq _ext@GOTPCREL(%rip), %rax'
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
grep -Eq "$t/got.o: _f\+0x[03]: .* to 0x200000000 \('_ext'\)" $t/got.log

# So is each stub's out of reach of its pointer, named after its
# symbol. (ld-prime reports the first stub's only, as a subsection of
# its own "stubs-got-file".)
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
grep -q "stub for _extf: .* to its pointer at 0x200000000$" $t/call.log
grep -q "stub for _extg: .* to its pointer at 0x200000008$" $t/call.log

# Every such error is reported. (ld-prime reports only the first in the
# output file, and prints the layout.)
if [ $ARCH = arm64 ]; then
  far='  bl _far'
  near='  adrp x0, _x@PAGE'
else
  far='  call _far'
  near='  movl _x(%rip), %eax'
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
not $mold -arch $ARCH -platform_version macos 14.0 14.0 -static -e start \
  -segaddr __A 0x100000000 -segaddr __TEXT 0x200000000 -segaddr __DATA 0x400000000 \
  -segaddr __FAR 0x600000000 $t/first1.o $t/first2.o -o $t/first 2> $t/first.log
[ "$(grep -c 'out of range' $t/first.log)" = 4 ]
grep -q "$t/first1.o: _a+0x" $t/first.log
[ "$(grep -c "$t/first2.o: _b+0x" $t/first.log)" = 2 ]
grep -q "$t/first2.o: _c+0x" $t/first.log

# A call to an import, or to a definition dyld may interpose, goes to
# its stub. (ld-prime names the stub ''. An arm64 thunk would reach the
# stub; -no_branch_islands leaves the branch out of reach.)
if [ $ARCH = arm64 ]; then
  call=bl
  late=0x110000000
  islands=-no_branch_islands
else
  call=call
  late=0x190000000
  islands=
fi
cat <<EOF | $CC -o $t/late.o -c -xassembler -
.text
.globl _main, _wk
.weak_definition _wk
.p2align 2
_main: ret
_wk: ret
.section __LATE,__text,regular,pure_instructions
.globl _late, _late2
.p2align 2
_late:
  $call _extf
_late2:
  $call _wk
.subsections_via_symbols
EOF
not $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" \
  -o $t/late $t/late.o $t/libext2.dylib -lSystem -segaddr __LATE $late $islands 2> $t/late.log
grep -Eq "$t/late.o: _late\+0x[01]: .* \('_extf'\)" $t/late.log

not $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" -dylib \
  -o $t/late.dylib $t/late.o $t/libext2.dylib -lSystem -segaddr __LATE $late $islands \
  2> $t/late.log
grep -Eq "$t/late.o: _late2\+0x[01]: .* \('_wk'\)" $t/late.log
