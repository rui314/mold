#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# An LDR or STR scales its 12-bit offset by its access size, so it can
# only reach a target that size divides. One that doesn't is an error
# naming the target; a load of a thread-local variable from a dylib goes
# through its 8-byte __got slot, named after the variable. (ld-prime
# names a GOT slot ''.)
cat <<EOF | $CC -shared -o $t/libfoo.dylib -xc -
_Thread_local long tlv1 = 1;
_Thread_local long tlv2 = 2;
EOF

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  adrp x0, _tlv1@TLVPPAGE
  ldr q0, [x0, _tlv1@TLVPPAGEOFF]
  adrp x0, _tlv2@TLVPPAGE
  ldr q0, [x0, _tlv2@TLVPPAGEOFF]
  mov w0, #0
  ret
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libfoo.dylib 2> $t/log
grep -qF "$t/a.o: _main+0xc: target '_tlv2' not 16-byte aligned, which is required by LDR/STR instruction" $t/log
not grep -q "_main+0x4:" $t/log

# An offset not paired with its adrp (its base register is another).
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  adrp x1, _d@PAGE
  ldr x0, [x2, _d@PAGEOFF]
  mov w0, #0
  ret
.data
.p2align 3
.long 0
_d: .quad 0
EOF

not $CC --ld-path=$mold -o $t/exe $t/b.o 2> $t/log
grep -qF "$t/b.o: _main+0x4: target '_d' not 8-byte aligned, which is required by LDR/STR instruction" $t/log
