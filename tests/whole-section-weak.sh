#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without .subsections_via_symbols a section is a single subsection,
# named by one symbol at the section's start (not the arm64 assembler's
# ltmpN): a non-weak one if there is any, else the last weak one. Only
# a weak symbol that names the subsection stops being weak.

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern long w;
int main() { printf("%ld\n", w); }
EOF

# _w alone at the start names the subsection.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.globl _w
.weak_definition _w
.p2align 3
_w: .quad 1
EOF

# A local label at the start names the subsection.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.p2align 3
local_start:
.globl _w
.weak_definition _w
_w: .quad 2
EOF

# So does a strong global.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
.p2align 3
.globl _a
_a:
.globl _w
.weak_definition _w
_w: .quad 3
EOF

# Nothing named at the start (x86-64 emits no ltmpN): no name at all.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.data
.p2align 3
.quad 0
.globl _w
.weak_definition _w
_w: .quad 4
EOF

# All weak: the last one, _w, names the subsection.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.data
.p2align 3
.globl _u
.weak_definition _u
_u:
.globl _v
.weak_definition _v
_v:
.globl _w
.weak_definition _w
_w: .quad 5
EOF

$CC --ld-path=$mold -o $t/exe-a $t/main.o $t/a.o
nm -m $t/exe-a > $t/syms-a
grep -q ') external _w$' $t/syms-a

# In b, c and d, _w is a weak label into the subsection: alone it
# stays weak, and a's plain _w overrides it.
for o in b c d; do
  $CC --ld-path=$mold -o $t/exe-$o $t/main.o $t/$o.o
  nm -m $t/exe-$o > $t/syms-$o
  grep -q ') weak external _w$' $t/syms-$o
  $CC --ld-path=$mold -o $t/exe-$o-a $t/main.o $t/$o.o $t/a.o
  $RUN $t/exe-$o-a | grep -q '^1$'
done

$CC --ld-path=$mold -o $t/exe-e $t/main.o $t/e.o
nm -m $t/exe-e > $t/syms-e
grep -q ') weak external _u$' $t/syms-e
grep -q ') weak external _v$' $t/syms-e
grep -q ') external _w$' $t/syms-e

# A .weak_def_can_be_hidden name becomes a hidden plain definition, so
# two copies collide; a -r output keeps it as is.
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
not $CC --ld-path=$mold -o $t/exe-hh $t/main.o $t/h1.o $t/h2.o 2> /dev/null

$mold -arch $ARCH -r $t/h1.o -o $t/h1r.o
nm -m $t/h1r.o > $t/syms-h1r
grep -q 'weak external automatically hidden' $t/syms-h1r
$mold -arch $ARCH -r $t/e.o -o $t/er.o
nm -m $t/er.o > $t/syms-er
grep -q ') weak external .*_v$' $t/syms-er
grep -Eq '\) external( \[no dead strip\])? _w$' $t/syms-er
