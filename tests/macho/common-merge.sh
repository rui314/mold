#!/bin/bash
source "$(dirname "$0")"/common.inc

# Tentative definitions (common symbols) of one name merge as mold
# merges them, whatever the input order: into the largest size, the
# greatest alignment and the most restrictive visibility of them all.
# ld-prime differs: the largest wins whole, with its alignment and its
# visibility, and of those of one size the first, so an object that
# asks for more alignment than the largest copy may get less.
cat <<EOF | $CC -fcommon -o $t/a.o -c -xc -
char a_pad;
int foo[4];
int bar;
int baz[4];
EOF
cat <<EOF | $CC -fcommon -o $t/b.o -c -xc -
#include <stdio.h>
#include <stdint.h>
#include <string.h>
extern int baz[4];
int foo[64];
int bar __attribute__((aligned(256)));
int main() {
  memset(foo, 0xff, sizeof(foo));
  printf("%d %lu\n", baz[0], (unsigned long)((uintptr_t)&bar % 256));
}
EOF

for order in "$t/a.o $t/b.o" "$t/b.o $t/a.o"; do
  $CC --ld-path=$mold -o $t/exe $order
  $RUN $t/exe > $t/out
  grep -q '^0 ' $t/out
  nm -m $t/exe > $t/log
  grep -q '(__DATA,__common) external _foo$' $t/log
  if $mold -v 2>&1 | grep -q mold-macho; then
    # bar is 256-aligned whichever copy comes first (ld-prime: 4).
    grep -q '^0 0$' $t/out
  fi
done

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.private_extern _p
.comm _p,8,3
.comm _e,8,3
.comm _t,8,2
.text
.globl _f
_f: ret
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.comm _p,4,2
.private_extern _e
.comm _e,4,2
.private_extern _t
.comm _t,8,4
.text
.globl _g
_g: ret
.subsections_via_symbols
EOF

for order in "$t/c.o $t/d.o" "$t/d.o $t/c.o"; do
  $CC --ld-path=$mold -shared -o $t/e.dylib $order
  nm -m $t/e.dylib > $t/log
  grep -q 'non-external (was a private external) _p$' $t/log
  if $mold -v 2>&1 | grep -q mold-macho; then
    # A private external copy makes the symbol one, however small or
    # late (ld-prime keeps _e external, and _t only if c.o comes
    # first).
    grep -q 'non-external (was a private external) _e$' $t/log
    grep -q 'non-external (was a private external) _t$' $t/log
  fi
done

# The 16 bytes aligned to 4 and the 8 aligned to 32 merge into 16
# bytes aligned to 32: __common, holding _pad and _v, is aligned so
# (ld-prime: to 4) and holds the 16 bytes.
cat <<EOF | $CC -o $t/f.o -c -xassembler -
.comm _pad,1,0
.comm _v,16,2
.text
.globl _h
_h: ret
.subsections_via_symbols
EOF
echo '.comm _v,8,5' | $CC -o $t/g.o -c -xassembler -
for order in "$t/f.o $t/g.o" "$t/g.o $t/f.o"; do
  $CC --ld-path=$mold -shared -o $t/h.dylib $order
  otool -l $t/h.dylib > $t/lc
  size=$(awk '$1 == "sectname" { s = $2 } s == "__common" && $1 == "size" { print $2 }' $t/lc)
  [ $((size)) -ge 17 ]
  if $mold -v 2>&1 | grep -q mold-macho; then
    [ "$(awk '$1 == "sectname" { s = $2 } s == "__common" && $1 == "align" { print $2 }' $t/lc)" = '2^5' ]
    nm $t/h.dylib > $t/syms
    [ $((0x$(awk '/ _v$/ { print $1 }' $t/syms) % 32)) = 0 ]
  fi
done

# A -r output keeps the merged tentative definition (mold's
# common-merge.sh).
$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
nm -m $t/r.o > $t/log
grep -Eq '^0*100 \(common\) .*external _foo$' $t/log
if $mold -v 2>&1 | grep -q mold-macho; then
  grep -Eq '^0*4 \(common\) \(alignment 2\^8\) external _bar$' $t/log
fi
