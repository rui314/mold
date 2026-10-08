#!/bin/bash
source "$(dirname "$0")"/common.inc

# A slot of an input __DATA,__got that is no plain pointer to a symbol -
# one with an addend, or to a place in a section - keeps its addend:
# the indirect symbol table names its symbol if dyld binds it, and
# INDIRECT_SYMBOL_LOCAL otherwise. Its slots are pointer-aligned
# whatever the input says.
if [ $ARCH = arm64 ]; then
  cat <<EOF > $t/a.s
.section __DATA,__got,non_lazy_symbol_pointers
Lp1: .quad _arr+8
Lp2: .quad _puts+8
.data
.globl _arr
.p2align 3
_arr: .quad 1, 2, 3
.text
.globl _get1, _get2, _get3
.p2align 2
_get1:
  adrp x8, Lp1@PAGE
  ldr x0, [x8, Lp1@PAGEOFF]
  ret
_get2:
  adrp x8, Lp2@PAGE
  ldr x0, [x8, Lp2@PAGEOFF]
  ret
_get3:
  adrp x8, Lp1@PAGE
  ldr x0, [x8, Lp1@PAGEOFF]
  ret
.subsections_via_symbols
EOF
else
  cat <<EOF > $t/a.s
.section __DATA,__got,regular
Lp1: .quad _arr+8
Lp2: .quad _puts+8
Lp3: .quad Lx
.data
.globl _arr
.p2align 3
_arr: .quad 1, 2
Lx: .quad 3
.text
.globl _get1, _get2, _get3
_get1:
  movq Lp1(%rip), %rax
  ret
_get2:
  movq Lp2(%rip), %rax
  ret
_get3:
  movq Lp3(%rip), %rax
  ret
.subsections_via_symbols
EOF
fi
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
long *get1(void);
char *get2(void);
long *get3(void);
int main() {
  printf("%ld %d %ld\n", *get1(), get2() == (char *)puts + 8, *get3());
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log
not grep -q 'too small' $t/log
if [ $ARCH = arm64 ]; then
  $RUN $t/exe | grep -q '^2 1 2$'
else
  $RUN $t/exe | grep -q '^2 1 3$'
fi

# The arm64 section is of non-lazy pointers, the x86-64 one regular.
if [ $ARCH = arm64 ]; then
  otool -Iv $t/exe > $t/indirect
  grep -A5 '(__DATA_CONST,__got)' $t/indirect > $t/got
  grep -q ' _puts$' $t/got
  grep -q ' LOCAL$' $t/got
fi
dyld_info -fixups $t/exe | grep -q ' bind .*/_puts + 0x8$'
