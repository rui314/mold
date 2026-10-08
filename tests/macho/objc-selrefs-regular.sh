#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime merges the entries of __objc_selrefs naming one selector and
# names none of them only where the section has the literal-pointer
# type the compilers give it. A regular or coalesced __objc_selrefs is
# data: two objects' entries for one selector both stay, and their
# labels too - a local one as a local symbol of the final image, by its
# own name in a -r output.
for i in 1 2; do
  if [ $ARCH = arm64 ]; then
    load="adrp x8, _sel$i@PAGE
  ldr x0, [x8, _sel$i@PAGEOFF]
  ret"
  else
    load="movq _sel$i(%rip), %rax
  retq"
  fi
  cat <<EOF | $CC -o $t/$i.o -c -xassembler -
.subsections_via_symbols
.text
.globl _f$i
.p2align 2
_f$i:
  $load
.section __TEXT,__objc_methname,cstring_literals
Lm$i: .asciz "foo"
.section __DATA,__objc_selrefs,regular,no_dead_strip
.p2align 3
_sel$i: .quad Lm$i
EOF
done
cat <<EOF | $CC -o $t/main.o -c -xc -
int main(void) { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/1.o $t/2.o
otool -l $t/exe | grep -A3 'sectname __objc_selrefs' | grep -q 'size 0x0*10$'
nm $t/exe > $t/nm
grep -q ' s _sel1$' $t/nm
grep -q ' s _sel2$' $t/nm

$mold -r -arch $ARCH -o $t/r.o $t/1.o $t/2.o
nm $t/r.o > $t/rnm
grep -q ' s _sel1$' $t/rnm
grep -q ' s _sel2$' $t/rnm
