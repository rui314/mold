#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime wants every pointer dyld fixes up 8-aligned, as the links of
# a fixup chain are words. For a deployment target it gives chained
# fixups by default, it warns of each atom aligned less than a pointer
# that holds one, and then of each unaligned pointer if the image has
# classic dyld info. With chained fixups, an arm64 link fails at the
# first one (the last of the first atom that has one); an x86-64 image
# gets classic dyld info instead. For an older target it says nothing.

# _r0 puts the pointers of _r1 and _r2 off 8-byte boundaries.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__foo
.globl _r0
_r0: .byte 1
.globl _r1
_r1: .quad _bar
.quad _bar
.globl _r2
_r2: .quad _bar
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <string.h>
int bar = 3;
extern char r1[], r2[];
int main() {
  void *p, *q, *s;
  memcpy(&p, r1, 8);
  memcpy(&q, r1 + 8, 8);
  memcpy(&s, r2, 8);
  return !(p == &bar && q == &bar && s == &bar);
}
EOF

a="(/.*/$t/a.o)"
if [ $ARCH = arm64 ]; then
  not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log
  grep -q "alignment (1) of atom '_r1' $a is too small and may result in unaligned pointers" $t/log
  grep -q "alignment (1) of atom '_r2' $a is too small" $t/log
  not grep -q "atom '_r0'" $t/log
  grep -q "pointer not aligned in '_r1'+0x8 $a$" $t/log
  not grep -q "pointer not aligned in '_r1' " $t/log
  not grep -q "pointer not aligned in '_r2'" $t/log
else
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log
  grep -q "alignment (1) of atom '_r1' $a is too small" $t/log
  grep -q 'disabling chained fixups because of unaligned pointers' $t/log
  grep -q "pointer not aligned in '_r1' $a$" $t/log
  grep -q "pointer not aligned in '_r1'+0x8 $a$" $t/log
  grep -q "pointer not aligned in '_r2' $a$" $t/log
  otool -l $t/exe > $t/lc
  grep -q LC_DYLD_INFO_ONLY $t/lc
  not grep -q LC_DYLD_CHAINED_FIXUPS $t/lc
  $t/exe
fi

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-no_fixup_chains 2> $t/log2
grep -q "alignment (1) of atom '_r1' $a is too small" $t/log2
not grep -q 'disabling chained fixups' $t/log2
grep -q "pointer not aligned in '_r1' $a$" $t/log2
grep -q "pointer not aligned in '_r2' $a$" $t/log2
$t/exe2

$mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/log3
not grep -q aligned $t/log3

cat <<EOF | $CC -o $t/c.o -c -xassembler - -mmacosx-version-min=11.0
.section __DATA,__foo
.globl _r0
_r0: .byte 1
.globl _r1
_r1: .quad _bar
.quad _bar
.globl _r2
_r2: .quad _bar
.subsections_via_symbols
EOF
echo 'int bar = 3; extern char r1[]; int main() { return !r1[0]; }' |
  $CC -o $t/e.o -c -xc - -mmacosx-version-min=11.0
$CC --ld-path=$mold -o $t/exe4 $t/c.o $t/e.o -mmacosx-version-min=11.0 2> $t/log4
not grep -q aligned $t/log4

# A CFString constant is aligned to a pointer whatever its section
# says; ld-prime warns of a section that says otherwise, in every
# object it reads.
cat <<EOF | $CC -o $t/f.o -c -xassembler -
.section __TEXT,__cstring,cstring_literals
L_s: .asciz "hi"
.section __DATA,__cfstring
.p2align 2
L_c: .quad ___CFConstantStringClassReference
.long 1992
.space 4
.quad L_s
.quad 2
.data
.globl _cf
.p2align 3
_cf: .quad L_c
EOF
echo 'extern void *cf; int main() { return !cf; }' | $CC -o $t/g.o -c -xc -
$CC --ld-path=$mold -o $t/exe5 $t/g.o $t/f.o -framework CoreFoundation 2> $t/log5
grep -q "section __DATA/__cfstring is not pointer aligned in /.*/$t/f.o$" $t/log5
not grep -q 'alignment (' $t/log5
$t/exe5
$mold -r -arch $ARCH -o $t/r.o $t/f.o 2> $t/log6
grep -q "section __DATA/__cfstring is not pointer aligned in /.*/$t/f.o$" $t/log6
rm -f $t/lib.a
ar rcs $t/lib.a $t/f.o
echo 'int main() { return 0; }' | $CC -o $t/h.o -c -xc -
$CC --ld-path=$mold -o $t/exe7 $t/h.o $t/lib.a 2> $t/log7
grep -q "section __DATA/__cfstring is not pointer aligned in /.*/$t/lib.a\[2\](f.o)$" $t/log7

# -unaligned_pointers turns those warnings into an error, which stops at
# the first unaligned pointer, or silences them (x86-64 still gives
# chained fixups up, saying so). An arm64 image with chained fixups
# fails all the same, with a warning if the option says warning.
$CC --ld-path=$mold -o $t/exe8 $t/a.o $t/b.o -Wl,-no_fixup_chains \
  -Wl,-unaligned_pointers,suppress 2> $t/log8
not grep -q aligned $t/log8
not $CC --ld-path=$mold -o $t/exe9 $t/a.o $t/b.o -Wl,-no_fixup_chains \
  -Wl,-unaligned_pointers,error 2> $t/log9
grep -q "warning: alignment (1) of atom '_r1' $a is too small" $t/log9
grep -q "pointer not aligned in '_r1' $a$" $t/log9
not grep -q 'warning: pointer not aligned' $t/log9
[ "$(grep -c 'pointer not aligned' $t/log9)" = 1 ]
$CC --ld-path=$mold -o $t/exe10 $t/c.o $t/e.o -mmacosx-version-min=11.0 \
  -Wl,-unaligned_pointers,warn 2> $t/log10
grep -q "pointer not aligned in '_r1' (" $t/log10
if [ $ARCH = arm64 ]; then
  not $CC --ld-path=$mold -o $t/exe11 $t/a.o $t/b.o -Wl,-unaligned_pointers,warning 2> $t/log11
  grep -q 'warning: unaligned pointer errors are fatal when using chained fixups' $t/log11
  grep -q "pointer not aligned in '_r1'+0x8 $a$" $t/log11
else
  $CC --ld-path=$mold -o $t/exe11 $t/a.o $t/b.o -Wl,-unaligned_pointers,suppress 2> $t/log11
  grep -v '^+' $t/log11 | sed -E 's/^(ld|mold): //' > $t/msgs11
  [ "$(cat $t/msgs11)" = 'warning: disabling chained fixups because of unaligned pointers' ]
fi
not $mold -o $t/exe12 $t/a.o -unaligned_pointers foo 2> $t/log12
grep -q -- '-unaligned_pointers invalid option (warning | error | suppress)' $t/log12
