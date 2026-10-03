#!/bin/bash
source "$(dirname "$0")"/common.inc

# -possible-lfoo, -possible_framework Foo and -possible_library path
# name a library as an auto-link option does, as a hint: it is looked
# at only after the command line's inputs, gets a load command only if
# something binds to it (by install name, with the auto-linked ones),
# and if it isn't there, the link says so only when symbols stay
# undefined. Another option naming the library makes it an input like
# any other.
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc -
echo 'int foo(void) { return 7; }' | $CC -o $t/foo2.o -c -xc -
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,/AAA/libfoo.dylib
mkdir -p $t/ar
rm -f $t/ar/libfoo.a $t/libfoo2.a
ar rcs $t/ar/libfoo.a $t/foo2.o
ar rcs $t/libfoo2.a $t/foo2.o
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/b.o -c -xc -

$CC --ld-path=$mold -o $t/exe1 $t/a.o -L$t -Wl,-possible-lfoo,-t > $t/trace
grep -q "libfoo.dylib" $t/trace
otool -L $t/exe1 > $t/libs1
not grep -q libfoo $t/libs1

$CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t -Wl,-possible-lfoo
otool -L $t/exe2 | tail -n +2 | awk '{print $1}' | tr '\n' ' ' > $t/libs2
[ "$(cat $t/libs2)" = "/usr/lib/libSystem.B.dylib /AAA/libfoo.dylib " ]

$CC --ld-path=$mold -o $t/exe3 $t/b.o -Wl,-possible_library,$t/libfoo.dylib $t/libfoo2.a
nm -m $t/exe3 | grep -q '(__TEXT,__text) external _foo$'
otool -L $t/exe3 > $t/libs3
not grep -q libfoo $t/libs3

$CC --ld-path=$mold -o $t/exe4 $t/b.o -L$t/ar -Wl,-possible-lfoo
nm -m $t/exe4 | grep -q '(__TEXT,__text) external _foo$'

$CC --ld-path=$mold -o $t/exe5 $t/a.o -L$t -Wl,-possible-lfoo,-lfoo
otool -L $t/exe5 | grep -q libfoo

$CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-possible_framework,None,-possible-lnone 2> $t/log6
not grep -q 'auto-linked' $t/log6

not $CC --ld-path=$mold -o $t/exe7 $t/b.o -Wl,-possible_framework,None,-possible-lnone 2> $t/log7
grep -q "Could not find or use auto-linked library 'none': library 'none' not found" $t/log7
grep -q "Could not find or use auto-linked framework 'None': framework 'None' not found" $t/log7

$CC --ld-path=$mold -o $t/exe8 $t/a.o -L$t \
  -Wl,-possible-lfoo,-possible-lfoo,-possible_library,$t/libfoo.dylib 2> $t/log8
grep -q "ignoring duplicate libraries: '.*foo'" $t/log8
