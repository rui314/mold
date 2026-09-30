#!/bin/bash
source "$(dirname "$0")"/common.inc

# A literal record a symbol labels stays an atom of its own, as in
# ld-prime: it merges with no identical record, labeled or not, and a
# -r output keeps its label, a plain local, where it names the other
# records LC<n> or l<nnn>. A linker-private (l) or temporary (L)
# label doesn't count; its record merges as an unlabeled one does. So
# it goes in objects with subsections and without alike.
cat > $t/a.s <<EOF
.text
.globl _f
.p2align 2
_f: ret
.section __TEXT,__cstring,cstring_literals
.asciz "x"
sa: .asciz "hello"
.asciz "hello"
sb: .asciz "hello"
lc: .asciz "hello"
.section __TEXT,__literal8,8byte_literals
.p2align 3
.quad 7
fa: .quad 1
.quad 1
fb: .quad 1
.data
.p2align 3
.globl _p
_p: .quad sa
  .quad sb
  .quad lc
  .quad fa
  .quad fb
EOF
$CC -o $t/a.o -c $t/a.s
cat $t/a.s > $t/a2.s
echo .subsections_via_symbols >> $t/a2.s
$CC -o $t/a2.o -c $t/a2.s

for obj in a a2; do
  $mold -arch $ARCH -r $t/$obj.o -o $t/r$obj.o
  nm -m $t/r$obj.o > $t/nm$obj
  grep -q '(__TEXT,__cstring) non-external sa$' $t/nm$obj
  grep -q '(__TEXT,__cstring) non-external sb$' $t/nm$obj
  grep -q '(__TEXT,__literal8) non-external fa$' $t/nm$obj
  grep -q '(__TEXT,__literal8) non-external fb$' $t/nm$obj
  not grep -q ' lc$' $t/nm$obj
  [ "$(grep -c ' s[ab]$' $t/nm$obj)" = 2 ]
  [ "$(grep ' s[ab]$' $t/nm$obj | cut -d' ' -f1 | sort -u | wc -l)" -eq 2 ]
  objdump -h $t/r$obj.o > $t/sect$obj
  grep -Eq ' __cstring\s+00000014\s' $t/sect$obj
  grep -Eq ' __literal8\s+00000020\s' $t/sect$obj
done

# Across objects too: another object's labeled copy stays apart, its
# unlabeled one merges, in -r and final links alike.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__cstring,cstring_literals
sc: .asciz "hello"
.asciz "hello"
.section __TEXT,__literal8,8byte_literals
.p2align 3
fc: .quad 1
.quad 1
.data
.p2align 3
.globl _q
_q: .quad sc
  .quad fc
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a2.o $t/b.o -o $t/rab.o
objdump -h $t/rab.o > $t/sectab
grep -Eq ' __cstring\s+0000001a\s' $t/sectab
grep -Eq ' __literal8\s+00000028\s' $t/sectab

$CC --ld-path=$mold -shared -o $t/c.dylib $t/a2.o $t/b.o
objdump -h $t/c.dylib > $t/sectc
grep -Eq ' __cstring\s+0000001a\s' $t/sectc
grep -Eq ' __const\s+00000028\s' $t/sectc
