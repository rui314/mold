#!/bin/bash
source "$(dirname "$0")"/common.inc

# A class reference may point at a class only a temporary label names:
# x86-64 relocations refer to it section-relatively. From macOS 15 on
# ld-prime folds it into the GOT all the same, and the load of the slot
# becomes a lea of the class.
class() {
  if [ $ARCH = arm64 ]; then
    load='adrp x8, _OBJC_CLASSLIST_REFERENCES_$_@PAGE
  ldr x0, [x8, _OBJC_CLASSLIST_REFERENCES_$_@PAGEOFF]'
    addr='adrp x0, LCls@PAGE
  add x0, x0, LCls@PAGEOFF'
  else
    load='movq _OBJC_CLASSLIST_REFERENCES_$_(%rip), %rax'
    addr='leaq LCls(%rip), %rax'
  fi
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.section __DATA,__objc_data
.p2align 3
$2
LCls: .quad 0, 0, 0, 0, 0
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
_OBJC_CLASSLIST_REFERENCES_\$_: .quad LCls
.text
.globl _get, _addr
.p2align 2
_get:
  $load
  ret
_addr:
  $addr
  ret
.subsections_via_symbols
EOF
}
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
void *get(void), *addr(void);
int main() { printf("%d\n", get() == addr()); }
EOF

class a ''
$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -mmacosx-version-min=15.0
$t/exe | grep -q '^1$'
otool -l $t/exe > $t/lc
not grep -q __objc_classrefs $t/lc

# A reference into the middle of a subsection keeps its slot: ld-prime
# would point the load at the subsection's start instead.
if $mold -v 2>&1 | grep -q mold-macho; then
  class b '.quad 7'
  $CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b.o -mmacosx-version-min=15.0
  $t/exe2 | grep -q '^1$'
  otool -l $t/exe2 | grep -q 'sectname __objc_classrefs'
fi
