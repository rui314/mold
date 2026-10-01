#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static image keeps a GOT slot for an absolute symbol, whose GOT
# load can't become an address computation. Without an indirect symbol
# table to name its slots, ld-prime makes its __got plain data - but a
# PIE's, which has the table, holds non-lazy symbol pointers.
if [ $ARCH = arm64 ]; then
  load='adrp x0, _abs@GOTPAGE
  ldr x0, [x0, _abs@GOTPAGEOFF]'
else
  load='movq _abs@GOTPCREL(%rip), %rax'
fi
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _main
.p2align 2
_main:
  $load
  ret
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _abs
_abs = 0x1234
EOF

got_flags() { otool -l $1 | grep -A10 'sectname __got' | awk '$1 == "flags" { print $2 }'; }

$mold -arch $ARCH -static -e _main $t/a.o $t/b.o -o $t/exe1
[ "$(got_flags $t/exe1)" = 0x00000000 ]

$mold -arch $ARCH -preload -e _main $t/a.o $t/b.o -o $t/exe2
[ "$(got_flags $t/exe2)" = 0x00000000 ]

$mold -arch $ARCH -static -pie -e _main $t/a.o $t/b.o -o $t/exe3
[ "$(got_flags $t/exe3)" = 0x00000006 ]
otool -l $t/exe3 | grep -q 'nindirectsyms 1$'
