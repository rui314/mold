#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without .subsections_via_symbols a section is a single subsection,
# and a weak definition in it stays weak, as mold keeps STB_WEAK: a
# strong definition overrides it, two copies coalesce, and an image
# exports it weak. ld-prime differs for the symbol that names the
# subsection, one at the section's start (not the arm64 assembler's
# ltmpN), which it makes non-weak, or hidden and non-weak if it is
# .weak_def_can_be_hidden, but in a -r output: two such copies collide,
# and so does one with a strong definition. A weak symbol after a
# local label or a strong one is weak in both.

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern long w;
int main() { printf("%ld\n", w); }
EOF

# _w alone at the start names the subsection.
for n in 1 2; do
  cat <<EOF | $CC -o $t/a$n.o -c -xassembler -
.data
.globl _w
.weak_definition _w
.p2align 3
_w: .quad $n
EOF
done

# A local label at the start names it.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.p2align 3
local_start:
.globl _w
.weak_definition _w
_w: .quad 3
EOF

# A strong definition, with subsections.
cat <<EOF | $CC -o $t/s.o -c -xassembler -
.data
.globl _w
.p2align 3
_w: .quad 9
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe-b $t/main.o $t/b.o
nm -m $t/exe-b > $t/syms-b
grep -q ') weak external _w$' $t/syms-b
$CC --ld-path=$mold -o $t/exe-bs $t/main.o $t/b.o $t/s.o
$RUN $t/exe-bs | grep -q '^9$'

# The losing copy's section holds another symbol, _pad, whose bytes
# stay.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
.globl _w, _pad
.weak_definition _w
.p2align 3
_w: .quad 4
_pad: .quad 5
EOF
cat <<EOF | $CC -o $t/pad.o -c -xc -
#include <stdio.h>
extern long w, pad;
int main() { printf("%ld %ld\n", w, pad); }
EOF

if $mold -v 2>&1 | grep -q mold-macho; then
  $CC --ld-path=$mold -o $t/exe-a $t/main.o $t/a1.o
  nm -m $t/exe-a > $t/syms-a
  grep -q ') weak external _w$' $t/syms-a

  $CC --ld-path=$mold -o $t/exe-aa $t/main.o $t/a1.o $t/a2.o
  $RUN $t/exe-aa | grep -q '^1$'
  $CC --ld-path=$mold -o $t/exe-as $t/main.o $t/a1.o $t/s.o
  $RUN $t/exe-as | grep -q '^9$'
  $CC --ld-path=$mold -o $t/exe-sa $t/main.o $t/s.o $t/a1.o
  $RUN $t/exe-sa | grep -q '^9$'

  $CC --ld-path=$mold -o $t/exe-ca $t/pad.o $t/a1.o $t/c.o
  $RUN $t/exe-ca | grep -q '^1 5$'

  # A dylib exports it weak, for dyld to coalesce.
  $CC --ld-path=$mold -shared -o $t/liba.dylib $t/a1.o
  nm -m $t/liba.dylib > $t/syms-dylib
  grep -q ') weak external _w$' $t/syms-dylib
fi

# A .weak_def_can_be_hidden one is hidden in an image (ld-prime makes
# it non-weak there, so two copies collide), and a -r output keeps it
# as is.
for n in 1 2; do
  cat <<EOF | $CC -o $t/h$n.o -c -xassembler -
.data
.globl _w
.weak_def_can_be_hidden _w
.p2align 3
_w: .quad $n
EOF
done
$CC --ld-path=$mold -o $t/exe-h $t/main.o $t/h1.o
nm -m $t/exe-h > $t/syms-h
grep -q 'non-external (was a private external) _w$' $t/syms-h
if $mold -v 2>&1 | grep -q mold-macho; then
  $CC --ld-path=$mold -o $t/exe-hh $t/main.o $t/h1.o $t/h2.o
  $RUN $t/exe-hh | grep -q '^1$'
fi

$mold -arch $ARCH -r $t/h1.o -o $t/h1r.o
nm -m $t/h1r.o > $t/syms-h1r
grep -q 'weak external automatically hidden' $t/syms-h1r
