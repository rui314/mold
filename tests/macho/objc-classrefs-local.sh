#!/bin/bash
source "$(dirname "$0")"/common.inc

# A class reference may point at a class only a temporary label names:
# an arm64 assembler's relocation names the section's ltmp label, an
# x86-64 one's the section and offset. From macOS 15 on, a slot that
# points at its class through a symbol folds into the GOT as any other
# (the load becomes an adrp+add of the class); one that points at it
# section-relatively, or into the middle of a subsection, stays in
# __objc_classrefs. (ld-prime folds those too, through symbols of its
# own, and points the load of one into a subsection's middle at the
# subsection's start.)
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
$RUN $t/exe | grep -q '^1$'
otool -l $t/exe > $t/lc
if [ $ARCH = arm64 ]; then
  not grep -q __objc_classrefs $t/lc
elif $mold -v 2>&1 | grep -q mold-macho; then
  grep -q 'sectname __objc_classrefs' $t/lc
fi

if $mold -v 2>&1 | grep -q mold-macho; then
  class b '.quad 7'
  $CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b.o -mmacosx-version-min=15.0
  $RUN $t/exe2 | grep -q '^1$'
  otool -l $t/exe2 | grep -q 'sectname __objc_classrefs'
fi
