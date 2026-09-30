#!/bin/bash
source "$(dirname "$0")"/common.inc

# A symbol of a thread-local zero-fill section (__thread_bss) names the
# initial storage of a thread-local variable, which only its own
# object's __thread_vars descriptor refers to. ld-prime makes an
# external one a plain local: another object's reference to it is
# undefined, another definition of it is no duplicate, and the output,
# -r too, lists it as a non-external symbol, not a private external.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.tbss _tbss_global, 8, 3
.globl _tbss_global
.tbss _tbss_pext, 8, 3
.globl _tbss_pext
.private_extern _tbss_pext
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.tbss _tbss_global, 8, 3
.globl _tbss_global
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/main.o
$t/exe
nm -m $t/exe > $t/syms
[ "$(grep -c '(__DATA,__thread_bss) non-external _tbss_global$' $t/syms)" = 2 ]
grep -q '(__DATA,__thread_bss) non-external _tbss_pext$' $t/syms
dyld_info -exports $t/exe > $t/exports
not grep -q _tbss $t/exports

cat <<EOF | $CC -o $t/c.o -c -xc -
extern char tbss_global;
char *get() { return &tbss_global; }
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/a.o $t/c.o $t/main.o 2> $t/log
grep -q _tbss_global $t/log

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
nm -m $t/r.o > $t/syms2
[ "$(grep -c '(__DATA,__thread_bss) non-external _tbss_global$' $t/syms2)" = 2 ]
grep -q '(__DATA,__thread_bss) non-external _tbss_pext$' $t/syms2
