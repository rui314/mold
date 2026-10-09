#!/bin/bash
source "$(dirname "$0")"/common.inc

# Hand-written arm64 code may reach a slot of its object's own GOT as
# another slot's label plus an offset: a local label's references are
# to the section's start (ltmpN) plus the offset. A mergeable dylib has
# each slot as an entry of its own, which a merging link places apart,
# so the reference must name the slot it reads. ld-prime records the
# first slot and the offset, and a program that merges its dylib reads
# the wrong slot: this test fails with ld-prime as $mold.
[ $ARCH = arm64 ] || skip

cat > $t/a.s <<EOF
.text
.globl _getsum
.p2align 2
_getsum:
  adrp x8, Lslots@PAGE
  ldr x8, [x8, Lslots@PAGEOFF]
  ldr w0, [x8]
  adrp x9, Lslots+8@PAGE
  ldr x9, [x9, Lslots+8@PAGEOFF]
  ldr w9, [x9]
  add w0, w0, w9
  adrp x10, Lslots+16@PAGE
  ldr x10, [x10, Lslots+16@PAGEOFF]
  ldr w10, [x10]
  add w0, w0, w10
  ret
.data
.globl _lvar
.p2align 2
_lvar: .long 100
.section __DATA,__got
.p2align 3
Lslots: .quad _lvar
  .quad _imp_var
  .quad _imp_var2
.subsections_via_symbols
EOF
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/imp.o -c -xc -
int imp_var = 7;
int imp_var2 = 30;
EOF
$CC -shared -o $t/libimp.dylib $t/imp.o -Wl,-install_name,$t/libimp.dylib

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int getsum(void);
int main() { printf("%d\n", getsum()); }
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -L$t -limp -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib

$CC --ld-path=$mold -o $t/exe1 $t/main.o -L$t -Wl,-merge-lfoo
$RUN $t/exe1 | grep -q '^137$'
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
$RUN $t/exe2 | grep -q '^137$'
