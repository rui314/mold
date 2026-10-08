#!/bin/bash
. $(dirname $0)/common.inc

# A section the linker synthesizes - the GOT, the stubs - keeps a
# section header of its own, with its own type, even when an input
# section (or a -sectcreate one, or a rename) has its name: the
# indirect symbol table still names its slots, and the input's
# contents stay where the input's symbols are. (ld-prime makes one
# section of the two instead, which takes the input's flags.)
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
$RUN $t/exe | grep '^2222 3 1$'
otool -l $t/exe > $t/load
[ "$(grep -c 'sectname __got' $t/load)" = 2 ]
[ "$(grep -c 'sectname __stubs' $t/load)" = 2 ]
grep -A10 'sectname __got' $t/load | grep -E 'flags 0x00000000$'
grep -A10 'sectname __got' $t/load | grep -E 'flags 0x00000006$'
grep -A10 'sectname __stubs' $t/load | grep -E 'flags 0x80000400$'
grep -A10 'sectname __stubs' $t/load | grep -E 'flags 0x80000408$'
otool -Iv $t/exe > $t/indirect
grep -A3 -F '(__DATA_CONST,__got)' $t/indirect | grep -F _malloc
grep -A2 -F '(__TEXT,__stubs)' $t/indirect | grep -F _printf

# Renamed into the GOT's name, an input section stays data.
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
$RUN $t/exe2 | grep '^3333 1$'
otool -Iv $t/exe2 > $t/indirect2
grep -A3 -F '(__DATA_CONST,__got)' $t/indirect2 | grep -F _malloc
nm -m $t/exe2 | grep -F '(__DATA_CONST,__got) external _fdata'

# So does a -sectcreate section of the name.
printf 'ABCDEFGH' > $t/blob
$CC --ld-path=$mold -o $t/exe3 $t/b.o -Wl,-sectcreate,__DATA_CONST,__got,$t/blob
$RUN $t/exe3 | grep '^3333 1$'
otool -Iv $t/exe3 > $t/indirect3
grep -A3 -F '(__DATA_CONST,__got)' $t/indirect3 | grep -F _malloc
off=$(otool -l $t/exe3 | awk '$1 == "sectname" { s = $2 } s == "__got" && $1 == "size" { z = $2 }
  s == "__got" && $1 == "offset" && z == "0x0000000000000008" { print $2 }')
dd if=$t/exe3 bs=1 skip=$off count=8 2> /dev/null > $t/blob3
cmp $t/blob $t/blob3

# A -sectcreate section of an input section's name joins it, after the
# input's contents.
$CC --ld-path=$mold -o $t/exe4 $t/b.o -Wl,-sectcreate,__DATA,__foo,$t/blob
$RUN $t/exe4 | grep '^3333 1$'
otool -l $t/exe4 > $t/load4
[ "$(grep -c 'sectname __foo' $t/load4)" = 1 ]
grep -A3 'sectname __foo' $t/load4 | grep -E 'size 0x0+10$'
otool -s __DATA __foo $t/exe4 > $t/foo
grep -E '00003333 00000000 44434241 48474645|33 33 00 00 00 00 00 00 41 42 43 44 45 46 47 48' $t/foo
