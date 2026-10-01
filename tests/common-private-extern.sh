#!/bin/bash
source "$(dirname "$0")"/common.inc

# A hidden tentative definition (-fcommon, visibility hidden) is a
# private-extern common symbol: N_UNDF | N_EXT | N_PEXT with the size
# in n_value. The linker allocates it in __common like any other
# common, and the output symbol table lists it as a local (nm's 's'),
# the way ld-prime lists LuaSkin's ____asan_globals_registered and
# _VPMergeHook; an earlier version of this linker dropped such symbols
# from the table because the synthesized __common section had no
# owning file.
cat <<EOF2 | $CC -fcommon -o $t/a.o -c -xc -
int __attribute__((visibility("hidden"))) hidden_common;
int visible_common;
int main() { return hidden_common + visible_common; }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe

nm -ap $t/exe > $t/nm
grep -q ' s _hidden_common$' $t/nm
grep -q ' S _visible_common$' $t/nm

# A -r output keeps it a private-extern common, -keep_private_externs
# or not: it allocates no commons, and so demotes none.
$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm2
grep -q '(common) (alignment 2^2) private external _hidden_common$' $t/nm2
grep -q '(common) (alignment 2^2) external _visible_common$' $t/nm2
