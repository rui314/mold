#!/bin/bash
source "$(dirname "$0")"/common.inc

# An object dead stripping left no symbol of gets no debug notes at
# all: ld-prime writes an object's N_SO/N_OSO run for the subsections
# it keeps, as for a ThinLTO module whose code was all inlined
# elsewhere.
echo 'int main() { return 0; }' | $CC -g -c -xc - -o $t/a.o
echo 'int unused(int x) { return x; }' | $CC -g -c -xc - -o $t/b.o
echo 'static int x = 5; int *unused2(void) { return &x; }' | $CC -g -c -xc - -o $t/c.o
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -Wl,-dead_strip
nm -ap $t/exe > $t/nm
grep -q ' OSO .*/a.o$' $t/nm
not grep -q -e ' OSO .*/b.o$' -e ' OSO .*/c.o$' $t/nm
[ $(grep -c ' OSO ' $t/nm) = 1 ]
