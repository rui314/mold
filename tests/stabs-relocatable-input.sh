#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output carries its objects' debug notes, and a final link copies
# them, but as ld-prime reads them: by the symbol an N_GSYM names. A
# private external, which the -r link demoted to a local, gets an
# N_STSYM of its address instead, as a local in a unit with DWARF
# would; an external keeps its N_GSYM, with no address. The notes of
# a weak definition another object's copy won are dropped with it.
cat <<EOF > $t/x.c
__attribute__((weak)) int wg = 1;
__attribute__((weak, visibility("hidden"))) int wh = 2;
__attribute__((weak)) int wf(void) { return 3; }
int xf(void) { return wg + wh + wf(); }
EOF
cat <<EOF > $t/y.c
__attribute__((weak)) int wg = 1;
__attribute__((weak, visibility("hidden"))) int wh = 2;
__attribute__((weak)) int wf(void) { return 3; }
int xf(void);
int main(void) { return xf() + wg + wh + wf() - 12; }
EOF
$CC -g -c $t/x.c -o $t/x.o
$CC -g -c $t/y.c -o $t/y.o
$mold -r -arch $ARCH -o $t/xr.o $t/x.o
$mold -r -arch $ARCH -o $t/yr.o $t/y.o
nm -ap $t/xr.o | grep -q ' GSYM _wh$'

$CC --ld-path=$mold -o $t/exe $t/xr.o $t/yr.o
$t/exe

nm -ap $t/exe > $t/stabs
[ "$(grep -c ' STSYM _wh$' $t/stabs)" = 2 ]
not grep -q ' GSYM _wh$' $t/stabs
grep -q '^0000000000000000 - 00 0000  GSYM _wg$' $t/stabs
[ "$(grep -c ' GSYM _wg$' $t/stabs)" = 1 ]
[ "$(grep -c ' FUN _wf$' $t/stabs)" = 1 ]

# A unit left with no notes goes, N_SO and N_OSO entries and all: here
# dead.o's only function is dead-stripped.
echo 'int live(void) { return 1; }' > $t/live.c
echo 'int dead(void) { return 2; }' > $t/dead.c
echo 'int live(void); int main(void) { return live() - 1; }' > $t/main.c
$CC -g -c $t/live.c -o $t/live.o
$CC -g -c $t/dead.c -o $t/dead.o
$CC -g -c $t/main.c -o $t/main.o
$mold -r -arch $ARCH -o $t/r.o $t/live.o $t/dead.o
grep -q dead.o $t/r.o
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/r.o -Wl,-dead_strip
$t/exe2
nm -ap $t/exe2 > $t/stabs2
grep -q ' OSO .*/live.o$' $t/stabs2
not grep -q 'dead' $t/stabs2
[ "$(grep -c ' SO $' $t/stabs2)" = 2 ]
