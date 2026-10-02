#!/bin/bash
source "$(dirname "$0")"/common.inc

# A literal record a symbol labels stays a subsection of its own, as in
# ld-prime: it merges with no identical record, labeled or not. A
# linker-private (l) or temporary (L) label doesn't count; its record
# merges as an unlabeled one does. So it goes in objects with
# subsections and without alike, and in a -r output, which keeps every
# record and label for the final link to merge.
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
  grep -q '(__TEXT,__cstring) non-external.* sa$' $t/nm$obj
  grep -q '(__TEXT,__cstring) non-external.* sb$' $t/nm$obj
  grep -q '(__TEXT,__literal8) non-external.* fa$' $t/nm$obj
  grep -q '(__TEXT,__literal8) non-external.* fb$' $t/nm$obj
  for in in $obj r$obj; do
    $CC --ld-path=$mold -shared -o $t/$in.dylib $t/$in.o
    objdump -h $t/$in.dylib > $t/sect$in
    grep -Eq ' __cstring\s+00000014\s' $t/sect$in
    grep -Eq ' __const\s+00000020\s' $t/sect$in
  done
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
for in in "$t/a2.o $t/b.o" $t/rab.o; do
  $CC --ld-path=$mold -shared -o $t/c.dylib $in
  objdump -h $t/c.dylib > $t/sectc
  grep -Eq ' __cstring\s+0000001a\s' $t/sectc
  grep -Eq ' __const\s+00000028\s' $t/sectc
done
