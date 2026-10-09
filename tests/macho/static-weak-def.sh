#!/bin/bash
source "$(dirname "$0")"/common.inc

# No dyld coalesces a -static image's weak definitions with another
# image's, so its code reaches them directly: a call branches to the
# definition and a GOT load becomes an address computation, leaving no
# __stubs or __got, as in ld-prime.
cat <<EOF | $CC -o $t/a.o -c -xassembler-with-cpp -
.text
.globl __start
.globl _wf
.weak_definition _wf
.p2align 2
__start:
#ifdef __arm64__
  bl _wf
  adrp x0, _wf@GOTPAGE
  ldr x0, [x0, _wf@GOTPAGEOFF]
#else
  call _wf
  movq _wf@GOTPCREL(%rip), %rax
#endif
  ret
_wf:
  ret
EOF

$mold -arch $ARCH -static -e __start $t/a.o -o $t/exe
otool -l $t/exe > $t/lc
not grep -q 'sectname __got' $t/lc
not grep -q 'sectname __stubs' $t/lc
otool -tv $t/exe > $t/dis
if [ $ARCH = arm64 ]; then
  not grep -q ldr $t/dis
else
  grep -q 'leaq' $t/dis
fi
