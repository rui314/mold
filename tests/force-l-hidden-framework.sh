#!/bin/bash
source "$(dirname "$0")"/common.inc

# -force-lfoo is -force_load of the library -lfoo finds, and
# -hidden_framework Foo and -load_hidden path are -hidden-l of a
# framework and of a path: an archive's members all load, or its
# definitions are hidden. Each merges with the other options that name
# the same file (or framework), as library options do: -force_load
# with a -hidden-l finding the archive at the same path loads it whole
# and hidden. A dylib named so links as any other.
echo 'int fa(void) { return 9; }' | $CC -o $t/fa.o -c -xc -
echo 'int fb(void) { return 8; }' | $CC -o $t/fb.o -c -xc -
mkdir -p $t/ar $t/Stat.framework
rm -f $t/ar/libfa.a
ar rcs $t/ar/libfa.a $t/fa.o $t/fb.o
cp $t/ar/libfa.a $t/Stat.framework/Stat
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc -
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
echo 'int fa(void); int main() { return fa(); }' | $CC -o $t/b.o -c -xc -
echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/c.o -c -xc -

$CC --ld-path=$mold -o $t/exe1 $t/a.o -L$t/ar -Wl,-force-lfa,-why_load 2> $t/log
nm -m $t/exe1 > $t/nm1
grep -q ' external _fa$' $t/nm1
grep -q ' external _fb$' $t/nm1
grep -q '^-force_load caused load of .*libfa.a.*(fa.o)' $t/log

$CC --ld-path=$mold -o $t/exe2 $t/a.o -L$t/ar -Wl,-force-l,fa,-hidden-lfa
nm -m $t/exe2 | grep -q 'non-external (was a private external) _fb$'

$CC --ld-path=$mold -o $t/exe3 $t/a.o -L$t/ar -Wl,-hidden-lfa,-force_load,$t/ar/libfa.a
nm -m $t/exe3 | grep -q 'non-external (was a private external) _fb$'

$CC --ld-path=$mold -o $t/exe4 $t/b.o -F$t -Wl,-hidden_framework,Stat
nm -m $t/exe4 > $t/nm4
grep -q 'non-external (was a private external) _fa$' $t/nm4
not grep -q _fb $t/nm4

$CC --ld-path=$mold -o $t/exe5 $t/b.o -F$t -Wl,-framework,Stat,-hidden_framework,Stat
nm -m $t/exe5 | grep -q 'non-external (was a private external) _fa$'

$CC --ld-path=$mold -o $t/exe6 $t/b.o -Wl,-load_hidden,$t/ar/libfa.a
nm -m $t/exe6 | grep -q 'non-external (was a private external) _fa$'

$CC --ld-path=$mold -o $t/exe7 $t/c.o -L$t -Wl,-force-lfoo,-load_hidden,$t/libfoo.dylib
otool -L $t/exe7 | grep -q libfoo.dylib

not $CC --ld-path=$mold -o $t/exe8 $t/a.o -Wl,-force-lnone 2> $t/log
grep -Fq "library 'none' not found" $t/log
not $CC --ld-path=$mold -o $t/exe8 $t/a.o -Wl,-hidden_framework,None 2> $t/log
grep -Fq "framework 'None' not found" $t/log
not $CC --ld-path=$mold -o $t/exe8 $t/a.o -Wl,-load_hidden,$t/none.a 2> $t/log
grep -Fq "library '$t/none.a' not found" $t/log

# -force-l and -load_hidden repeat like the other library options;
# frameworks never do.
$CC --ld-path=$mold -o $t/exe9 $t/a.o -L$t/ar -F$t -Wl,-force-lfa,-force-lfa \
  -Wl,-load_hidden,$t/ar/libfa.a,-load_hidden,$t/ar/libfa.a \
  -Wl,-hidden_framework,Stat,-hidden_framework,Stat 2> $t/log
grep -Fq "ignoring duplicate libraries: '-force-lfa', '-load_hidden $t/ar/libfa.a'" $t/log
