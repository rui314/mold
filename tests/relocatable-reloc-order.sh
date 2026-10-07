#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output lists each section's relocations subsection by
# subsection in input order, keeping a pair - a SUBTRACTOR and its
# UNSIGNED, an arm64 ADDEND and the PAGE21 or PAGEOFF12 it goes with -
# together and in order, so that a later link reads each as the input
# had it. (ld-prime lists each subsection's by descending offset.)
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _hp
.p2align 2
_hp:
  adrp x0, _h@PAGE+16
  add x0, x0, _h@PAGEOFF+16
  ret
.globl _f2
_f2:
  b _ext3
.data
.globl _g
.p2align 3
_g: .quad _ext1
  .quad _ext2
  .quad _f2 - _g
  .quad _ext3
.globl _h
_h: .quad 0, 0, 0
.subsections_via_symbols
EOF
  sub=1
else
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _hp
_hp:
  leaq _h+16(%rip), %rax
  retq
.globl _f2
_f2:
  jmp _ext3
.data
.globl _g
.p2align 3
_g: .quad _ext1
  .quad _ext2
  .quad _f2 - _g
  .quad _ext3
.globl _h
_h: .quad 0, 0, 0
.subsections_via_symbols
EOF
  sub=5
fi

$mold -arch $ARCH -r $t/a.o -o $t/r.o
otool -r $t/r.o | awk '/^0/ { print substr($1, 7), $5 }' > $t/relocs
# Each SUBTRACTOR, and each ADDEND, comes just before the relocation at
# its address that it goes with.
awk -v sub_=$sub '
  pending != "" { if ($1 != pending) exit 1; pending = "" }
  $2 == sub_ || (sub_ == 1 && $2 == 10) { pending = $1 }
  END { if (pending != "") exit 1 }' $t/relocs
[ "$(grep -c " $sub\$" $t/relocs)" = 1 ]
if [ $ARCH = arm64 ]; then
  [ "$(grep -c ' 10$' $t/relocs)" = 2 ]
fi

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern long g[];
extern char h[];
char *hp(void);
int f2(void);
int ext1, ext2;
int ext3(void) { return 3; }
int main() {
  printf("%d %d %d %d %d\n", g[0] == (long)&ext1, g[1] == (long)&ext2,
         g[2] == (char *)f2 - (char *)g, hp() == h + 16, f2());
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$RUN $t/exe | grep -q '^1 1 1 1 3$'
$CC -o $t/exe2 $t/main.o $t/r.o
$RUN $t/exe2 | grep -q '^1 1 1 1 3$'
