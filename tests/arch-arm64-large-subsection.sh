#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# A code subsection larger than a branch reaches leaves no place for a
# thunk between its ends: a branch across it goes through a thunk placed
# before it, which jumps anywhere within 4 GiB. (ld-prime's branch
# islands branch to one another by b, so none crosses such a
# subsection: it warns about the subsection and fails the link.)
cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  stp x29, x30, [sp, #-16]!
  bl _far
  ldp x29, x30, [sp], #16
  ret
.globl _big
.p2align 2
_big:
  .space 0x8400000
.globl _far
.p2align 2
_far:
  mov w0, #42
  ret
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log
not grep -q warning $t/log
nm $t/exe | grep -q ' _far\.island$'
status=0
$t/exe || status=$?
[ $status = 42 ]
rm -f $t/a.o $t/exe
