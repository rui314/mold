#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime checks some sections of every object it parses. A label at
# the end of a section of fixed-size records names no record: it is
# ignored with a warning, and a relocation can't refer to it.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.quad 1
.globl _end8
_end8:
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _main
_endinit:
.subsections_via_symbols
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

$CC --ld-path=$mold -o $t/exe1 $t/main.o $t/a.o 2> $t/log1
grep -q "warning: ignoring extranenous label '_end8' at end of section '__literal8'" $t/log1
grep -q "warning: ignoring extranenous label '_endinit' at end of section '__mod_init_func'" $t/log1
$t/exe1
nm $t/exe1 > $t/syms1
not grep -q '_end8\|_endinit' $t/syms1

$mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/log2
grep -q "warning: ignoring extranenous label '_end8'" $t/log2
nm $t/r.o > $t/syms2
not grep -q '_end8\|_endinit' $t/syms2

# So is the ltmpN label the arm64 assembler puts at an empty section's
# start, in an object without subsections, where such labels count.
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
nop
.section __TEXT,__literal8,8byte_literals
EOF
  $mold -r -arch arm64 -o $t/r2.o $t/d.o 2> $t/log5
  grep -q "warning: ignoring extranenous label 'ltmp1' at end of section '__literal8'" $t/log5
  nm $t/r2.o > $t/syms5
  not grep -q ltmp1 $t/syms5
fi

# Its symbol table ends at the last symbol it keeps, so the index of a
# label no kept symbol follows is out of range. (The arm64 assembler
# adds ltmp0-2 at the sections' starts.)
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.quad 1
_end8:
.data
.p2align 3
.globl _p
_p: .quad _end8
EOF
[ $ARCH = arm64 ] && n=2 || n=0
not $CC --ld-path=$mold -o $t/exe3 $t/main.o $t/b.o 2> $t/log3
grep -q "invalid r_symbolnum=$n in '.*$t/b.o'" $t/log3

cat <<EOF | $CC -o $t/b2.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.quad 1
.globl _end8
_end8:
.data
.p2align 3
.quad _end8
EOF
[ $ARCH = arm64 ] && n=3 || n=0
not $CC --ld-path=$mold -o $t/exe3 $t/main.o $t/b2.o 2> $t/log3
grep -q "r_symbolnum=$n out of range in '.*$t/b2.o'" $t/log3

# An initializer or terminator pointer needs a relocation.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__mod_term_func,mod_term_funcs
.p2align 3
.quad 0
EOF
not $CC --ld-path=$mold -o $t/exe4 $t/main.o $t/c.o 2> $t/log4
grep -q "initializer pointer has no target in '/.*/$t/c.o'" $t/log4

# So does each entry of __objc_clsrolist, the list of Swift's class_ro_t
# records, which is a subsection of its own. (ld-prime has the check,
# but an assertion that the entry has a relocation trips first.)
if $mold -v 2>&1 | grep -q mold-macho; then
  cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __DATA,__objc_const
.p2align 3
_ro: .space 72
.section __DATA,__objc_clsrolist,regular,no_dead_strip
.p2align 3
.quad _ro
.quad 0
EOF
  not $CC --ld-path=$mold -o $t/exe5 $t/main.o $t/e.o 2> $t/log5
  grep -q "__objc_clsrolist pointer has no target in '/.*/$t/e.o'" $t/log5
fi

# An __objc_imageinfo record is 8 bytes; ld-prime ignores a shorter one
# (silently if empty) and reads the first 8 bytes of a longer one.
imageinfo() {
  echo '.section __DATA,__objc_imageinfo,regular,no_dead_strip'
  echo '.p2align 2'
  [ $1 -ge 4 ] && echo '.long 0'
  [ $1 -ge 8 ] && echo '.long 64'
  [ $1 -ge 12 ] && echo '.long 7'
  echo '.subsections_via_symbols'
}
for n in 0 4 12; do
  imageinfo $n | $CC -o $t/ii$n.o -c -xassembler -
  $CC --ld-path=$mold -o $t/exe-ii$n $t/main.o $t/ii$n.o 2> $t/log-ii$n
  $mold -r -arch $ARCH -o $t/r-ii$n.o $t/ii$n.o 2> $t/logr-ii$n
done
not grep -q . $t/log-ii0
grep -q "warning: can't parse __DATA/__objc_imageinfo section in /.*/$t/ii4.o" $t/log-ii4
grep -q "warning: can't parse __DATA/__objc_imageinfo section" $t/logr-ii4
grep -q "warning: section __DATA/__objc_imageinfo has unexpectedly large size 12 in /.*/$t/ii12.o" $t/log-ii12
grep -q "unexpectedly large size 12" $t/logr-ii12

otool -l $t/r-ii4.o > $t/lc4
not grep -q __objc_imageinfo $t/lc4
otool -s __DATA __objc_imageinfo $t/r-ii12.o > $t/sect12
grep -Eq '^0+	(00000000 00000040|00 00 00 00 40 00 00 00) $' $t/sect12
