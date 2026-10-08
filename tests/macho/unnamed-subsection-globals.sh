#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime splits UTF-16 literals, selector references and the like
# into subsections by content and names none of them, so an external
# symbol that labels one defines nothing: the object's own references
# reach it, an output lists it as a local at most, another object's
# reference to it is undefined, and another definition of the name is
# no duplicate. (ld-prime lists it nowhere.)
if [ $ARCH = arm64 ]; then
  cat > $t/a.s <<'EOF'
.text
.globl _main
.p2align 2
_main:
  adrp x0, _ustr@PAGE
  add x0, x0, _ustr@PAGEOFF
  adrp x1, _ustr_pe@PAGE
  add x1, x1, _ustr_pe@PAGEOFF
  adrp x2, _selref@PAGE
  ldr x2, [x2, _selref@PAGEOFF]
  mov w0, #0
  ret
EOF
else
  cat > $t/a.s <<'EOF'
.text
.globl _main
_main:
  leaq _ustr(%rip), %rax
  leaq _ustr_pe(%rip), %rax
  movq _selref(%rip), %rax
  xorl %eax, %eax
  ret
EOF
fi

cat >> $t/a.s <<'EOF'
.section __TEXT,__ustring
.globl _ustr
_ustr:
  .short 0x48, 0x69, 0
.globl _ustr_pe
.private_extern _ustr_pe
_ustr_pe:
  .short 0x48, 0x6f, 0
.section __TEXT,__objc_methname,cstring_literals
L_sel:
  .asciz "foo"
.section __DATA,__objc_selrefs,literal_pointers,no_dead_strip
.p2align 3
.globl _selref
_selref:
  .quad L_sel
.subsections_via_symbols
EOF

cat > $t/b.s <<'EOF'
.data
.p2align 3
.globl _ref
_ref:
  .quad _ustr
.subsections_via_symbols
EOF

cat > $t/c.s <<'EOF'
.data
.p2align 3
.globl _selref
_selref:
  .quad 7
.subsections_via_symbols
EOF

$CC -c -o $t/a.o $t/a.s
$CC -c -o $t/b.o $t/b.s
$CC -c -o $t/c.o $t/c.s

$CC --ld-path=$mold -o $t/exe1 $t/a.o
$RUN $t/exe1
nm -m $t/exe1 > $t/log1
grep -q '(__TEXT,__ustring) non-external _ustr$' $t/log1
not grep -q ') external _ustr\|) external _selref' $t/log1

not $CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o 2> $t/log2
grep -q '_ustr' $t/log2

$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/c.o
nm -m $t/exe3 | grep -q '(__DATA,__data) external _selref'

not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-u,_ustr 2> $t/log4
grep -q '_ustr' $t/log4

# A -r output keeps them as the non-external symbols they now are: it
# defines nothing by those names either, and its own references still
# reach them.
$CC --ld-path=$mold -r -o $t/d.o $t/a.o
nm -g $t/d.o > $t/log5
not grep -q '_ustr\|_selref' $t/log5
$CC --ld-path=$mold -o $t/exe5 $t/d.o
$RUN $t/exe5
