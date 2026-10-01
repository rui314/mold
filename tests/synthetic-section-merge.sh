#!/bin/bash
. $(dirname $0)/common.inc

# ld-prime makes one output section of each name: a section it
# synthesizes - the GOT, the stubs - joins an input section's of the
# same name (after renames), after the input's contents and with its
# flags, so the GOT or the stubs there are no longer typed as such.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
#include <stdlib.h>
__attribute__((section("__DATA_CONST,__got"))) long gdata[2] = {0x1111, 0x2222};
__attribute__((section("__TEXT,__stubs,regular,pure_instructions")))
int three(void) { return 3; }
int main() {
  void *(*p)(size_t) = malloc;
  printf("%lx %d %d\n", gdata[1], three(), p != 0);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '^2222 3 1$'
otool -l $t/exe > $t/load
[ "$(grep -c 'sectname __got' $t/load)" = 1 ]
[ "$(grep -c 'sectname __stubs' $t/load)" = 1 ]
grep -A10 'sectname __got' $t/load | grep -E 'flags 0x00000000$'
grep -A10 'sectname __stubs' $t/load | grep -E 'flags 0x80000400$'
# gdata comes first, the GOT's slots after it.
addr=$(grep -A3 'sectname __got' $t/load | awk '/addr/ { print $2 }')
nm -m $t/exe | grep -E "^0*${addr#0x} \(__DATA_CONST,__got\) external _gdata$"

# Renamed into the GOT's name, an input section takes the GOT in.
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
#include <stdlib.h>
__attribute__((section("__DATA,__foo"))) long fdata = 0x3333;
int main() {
  void *(*p)(size_t) = malloc;
  printf("%lx %d\n", fdata, p != 0);
}
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-rename_section,__DATA,__foo,__DATA_CONST,__got
$t/exe2 | grep '^3333 1$'
otool -l $t/exe2 > $t/load2
[ "$(grep -c 'sectname __got' $t/load2)" = 1 ]
otool -Iv $t/exe2 > $t/indirect
not grep -F '(__DATA_CONST,__got)' $t/indirect

# A -sectcreate section of the name comes first, then the linker's
# contents: the GOT's slots, or the lazy binder's __dyld_private word.
printf 'ABCDEFGH' > $t/blob
$CC --ld-path=$mold -o $t/exe3 $t/b.o -Wl,-sectcreate,__DATA_CONST,__got,$t/blob
$t/exe3 | grep '^3333 1$'
otool -l $t/exe3 > $t/load3
[ "$(grep -c 'sectname __got' $t/load3)" = 1 ]
grep -A3 'sectname __got' $t/load3 | grep -E 'size 0x0+18$'
otool -s __DATA_CONST __got $t/exe3 | grep -E '44434241 48474645|41 42 43 44 45 46 47 48'

$CC --ld-path=$mold -o $t/exe4 $t/b.o -mmacos-version-min=11.0 \
  -Wl,-no_fixup_chains,-sectcreate,__DATA,__data,$t/blob
$t/exe4 | grep '^3333 1$'
otool -l $t/exe4 > $t/load4
[ "$(grep -c 'sectname __data' $t/load4)" = 1 ]
grep -A3 'sectname __data' $t/load4 | grep -E 'size 0x0+10$'
nm -m $t/exe4 | grep -F '(__DATA,__data) non-external __dyld_private'
