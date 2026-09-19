#!/bin/bash
source "$(dirname "$0")"/common.inc

# In a -r link, private external symbols (visibility hidden) become
# plain non-external symbols unless -keep_private_externs. Apple's
# strip relies on this: `strip -S` on an archive runs the toolchain's
# `ld -r -keep_private_externs -S` on each member, so a linker without
# the option makes strip fail ("internal link edit command failed",
# seen on Sparkle's libbsdiff.a).
cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((visibility("hidden"))) int hidden_fn(void) { return 1; }
int visible_fn(void) { return hidden_fn() + 1; }
EOF
nm -m $t/a.o | grep -q 'private external _hidden_fn'

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm
# ld64 keeps N_PEXT on the demoted symbol (nm: "was a private external").
grep -q 'non-external (was a private external) _hidden_fn' $t/nm
grep -q ' external _visible_fn' $t/nm

$mold -r -arch $ARCH -keep_private_externs -o $t/k.o $t/a.o
nm -m $t/k.o | grep -q 'private external _hidden_fn'

# Both still link and work.
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int visible_fn(void);
int main() { printf("%d\n", visible_fn()); }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$t/exe | grep -q '^2$'
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/k.o
$t/exe2 | grep -q '^2$'

# The way strip drives it.
$mold -keep_private_externs -r -S $t/a.o -o $t/s.o
nm -m $t/s.o | grep -q 'private external _hidden_fn'
