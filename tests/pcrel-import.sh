#!/bin/bash
source "$(dirname "$0")"/common.inc

# A PC-relative reference that goes through no stub or GOT slot - an
# arm64 adrp or the offset into its page, an x86-64 RIP-relative one -
# needs its target's address, which an import has none of in the
# image: ld-prime fails the link, in a dylib as in an executable.
cat <<EOF | $CC -o $t/a.o -c -xc -
long x = 5;
EOF
$CC --ld-path=$mold -shared -o $t/libx.dylib $t/a.o

if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f
_f:
  adrp x0, _x@PAGE
  ldr x0, [x0, _x@PAGEOFF]
  ret
EOF
  cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _f
_f:
  ldr x0, [x0, _x@PAGEOFF]
  ret
EOF
else
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f
_f:
  leaq _x(%rip), %rax
  ret
EOF
  cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _f
_f:
  movl \$1, _x(%rip)
  ret
EOF
fi

for obj in b c; do
  not $CC --ld-path=$mold -shared -o $t/$obj.dylib $t/$obj.o $t/libx.dylib 2> $t/log
  grep -q "$t/$obj.o: _f+0x[0-9a-f]*: target '_x' does not have address" $t/log
  not $CC --ld-path=$mold -o $t/$obj.exe $t/$obj.o $t/libx.dylib -Wl,-e,_f 2> $t/log
  grep -q "$t/$obj.o: _f+0x[0-9a-f]*: target '_x' does not have address" $t/log
done
