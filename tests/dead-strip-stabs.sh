#!/bin/bash
source "$(dirname "$0")"/common.inc

# An object dead stripping left no symbol of keeps its N_SO/N_OSO run,
# with no notes in it: dsymutil maps nothing from it. (ld-prime writes
# the run only for an object some of whose subsections it keeps.)
echo 'int main() { return 0; }' | $CC -g -c -xc - -o $t/a.o
echo 'int unused(int x) { return x; }' | $CC -g -c -xc - -o $t/b.o
echo 'static int x = 5; int *unused2(void) { return &x; }' | $CC -g -c -xc - -o $t/c.o
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -Wl,-dead_strip
nm -ap $t/exe > $t/nm
grep -q ' OSO .*/a.o$' $t/nm
grep -q ' OSO .*/b.o$' $t/nm
grep -q ' OSO .*/c.o$' $t/nm
not grep -q -e ' FUN _unused' -e ' STSYM _x$' $t/nm
dsymutil -o $t/exe.dSYM $t/exe > $t/log 2>&1
not grep -qi warning $t/log
