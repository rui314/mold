#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# Every code section gets range-extension thunks, whatever its segment
# and the output type: a -static executable's __TEXT code, a kext's
# __TEXT_EXEC and a custom code segment alike. (ld-prime makes its
# branch islands only among the code of __TEXT, and fails the kext and
# __FOO links below with a fixup error.)
objs() {
  cat <<EOF | $CC -o $t/main.o -c -xassembler -
.subsections_via_symbols
.section $1,regular,pure_instructions
.globl _main
.p2align 2
_main:
  stp x29, x30, [sp, #-16]!
  bl _far
  ldp x29, x30, [sp], #16
  ret
EOF

  cat <<EOF | $CC -o $t/pad.o -c -xassembler -
.subsections_via_symbols
.section $1,regular,pure_instructions
_pad1:
  .long 1
  .space $2 - 4
_pad2:
  .long 2
  .space $3 - 4
EOF

  cat <<EOF | $CC -o $t/far.o -c -xassembler -
.subsections_via_symbols
.section $1,regular,pure_instructions
.globl _far
.p2align 2
_far:
  mov w0, #42
  ret
EOF
}

objs __TEXT,__text 0x4400000 0x4400000
$mold -arch $ARCH -static -e _main -o $t/exe1 $t/main.o $t/pad.o $t/far.o
nm $t/exe1 | grep -q ' _far\.island$'
rm -f $t/exe1

$mold -arch $ARCH -kext -o $t/kext $t/main.o $t/pad.o $t/far.o
nm -m $t/kext > $t/syms2
grep -q '(__TEXT_EXEC,__text) non-external _far\.island$' $t/syms2
rm -f $t/kext

# (-segprot makes __FOO executable, which a segment of a name the
# linker doesn't know is not otherwise.)
objs __FOO,__text 0x8400000 0x4000
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/pad.o $t/far.o -Wl,-segprot,__FOO,rx,rx
nm -m $t/exe3 | grep -q '(__FOO,__text) non-external _far\.island$'
status=0
$RUN $t/exe3 || status=$?
[ $status = 42 ]
rm -f $t/exe3

# A branch from __TEXT to the code of another segment out of its reach
# goes through a thunk in __TEXT.
cat <<EOF | $CC -o $t/main.o -c -xassembler -
.subsections_via_symbols
.text
.globl _main
.p2align 2
_main:
  stp x29, x30, [sp, #-16]!
  bl _far
  ldp x29, x30, [sp], #16
  ret
EOF
$CC --ld-path=$mold -o $t/exe4 $t/main.o $t/pad.o $t/far.o -Wl,-segprot,__FOO,rx,rx
nm -m $t/exe4 | grep -q '(__TEXT,__text) non-external _far\.island$'
status=0
$RUN $t/exe4 || status=$?
[ $status = 42 ]
rm -f $t/pad.o $t/exe4
