#!/bin/bash
source "$(dirname "$0")"/common.inc

# Bitcode clang built for ThinLTO (-flto=thin) goes through libLTO's
# thinlto_* API, which optimizes each module on its own with a summary
# of all of them and compiles it to an object of its own; the other
# bitcode is merged into one module and object, as before.
cat <<EOF | $CC -flto=thin -g -o $t/a.o -c -xc -
#include <stdio.h>
int foo(int);
int bar(int);
int main(int argc, char **argv) { printf("%d %d\n", foo(argc), bar(argc)); }
EOF
cat <<EOF | $CC -flto=thin -g -o $t/b.o -c -xc -
int baz(int);
int foo(int x) { return baz(x) + 1; }
EOF
cat <<EOF | $CC -flto -o $t/c.o -c -xc -
int baz(int x) { return x * 7; }
EOF
cat <<EOF | $CC -o $t/d.o -c -xc -
int bar(int x) { return x - 1; }
EOF
cat <<EOF | $CC -o $t/e.o -c -xc -
int baz(int x) { return x * 7; }
EOF

# ld-prime leaves the ThinLTO objects nameless: the map lists them after
# the dylibs, ahead of the merged modules' /tmp/lto.o, and their debug
# stabs name no file, with modification time 0.
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o -Wl,-map,$t/map
$t/exe | grep -q '^8 0$'
sed -n '/^# Object files:/,/^# Sections:/p' $t/map | grep '^\[' > $t/files
[ "$(tail -3 $t/files | sed 's/^\[ *[0-9]*\] //')" = "$(printf '\n\n/tmp/lto.o')" ]
nm -ap $t/exe | grep -q '^0000000000000000 - .. 0001   OSO $'

# With -object_path_lto, libLTO writes them to that directory as
# <index>.<arch>.thinlto.o, and they go by those names; the merged
# modules' object goes there as lto.o.
rm -rf $t/objs
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o $t/c.o $t/d.o -Wl,-object_path_lto,$t/objs \
  -Wl,-map,$t/map2
$t/exe2 | grep -q '^8 0$'
otool -hv $t/objs/1.$ARCH.thinlto.o | grep -q OBJECT
grep -q "^\[ *[0-9]*\] $t/objs/0.$ARCH.thinlto.o\$" $t/map2
grep -q "^\[ *[0-9]*\] $t/objs/lto.o\$" $t/map2
nm -ap $t/exe2 | grep -q " OSO $(pwd)/$t/objs/0.$ARCH.thinlto.o\$"

# -save-temps keeps ThinLTO's bitcode at each stage in the directory
# <output>.thinlto.bcs, and the objects as <output>.<index>.thinlto.o,
# besides the merged modules' <output>.lto.bc, .lto.opt.bc and .lto.o.
rm -rf $t/exe7*
$CC --ld-path=$mold -o $t/exe7 $t/a.o $t/b.o $t/c.o $t/d.o -Wl,-save-temps
$t/exe7 | grep -q '^8 0$'
otool -hv $t/exe7.1.thinlto.o | grep -q OBJECT
[ -s $t/exe7.thinlto.bcs/0.4.opt.bc ]
[ -s $t/exe7.lto.bc ]

# -flto-codegen-only has ThinLTO compile every module as it is, the ones
# to merge too, without optimizing: an object each, and no /tmp/lto.o.
$CC --ld-path=$mold -o $t/exe8 $t/a.o $t/b.o $t/c.o $t/d.o -Wl,-flto-codegen-only \
  -Wl,-map,$t/map8
$t/exe8 | grep -q '^8 0$'
sed -n '/^# Object files:/,/^# Sections:/p' $t/map8 | grep '^\[' > $t/files8
[ $(grep -c '^\[ *[0-9]*\] $' $t/files8) = 3 ]
not grep -q /tmp/lto.o $t/files8

# ThinLTO bitcode in an archive, and in a dylib, which keeps its exports.
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/b.o
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/libfoo.a $t/e.o $t/d.o
$t/exe3 | grep -q '^8 0$'
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/b.o $t/c.o
dyld_info -exports $t/libfoo.dylib | grep -q ' _foo$'
$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/d.o $t/libfoo.dylib
$t/exe4 | grep -q '^8 0$'

# A -r link of ThinLTO bitcode compiles it, whatever else it takes:
# only merged modules can be written out as bitcode.
lto_library=$(dirname "$(xcrun -f clang)")/../lib/libLTO.dylib
$mold -arch $ARCH -r -lto_library $lto_library -o $t/r.o $t/a.o $t/b.o
otool -hv $t/r.o | grep -q OBJECT
$CC --ld-path=$mold -o $t/exe5 $t/r.o $t/e.o $t/d.o
$t/exe5 | grep -q '^8 0$'

# -mllvm and -mcpu reach ThinLTO too.
not $CC --ld-path=$mold -o $t/exe6 $t/a.o $t/b.o $t/e.o $t/d.o -Wl,-mllvm,-bogus-option \
  2> $t/log
grep -q "Unknown command line argument '-bogus-option'" $t/log
$CC --ld-path=$mold -o $t/exe6 $t/a.o $t/b.o $t/e.o $t/d.o -Wl,-mcpu,bogus 2> $t/log
grep -q "'bogus' is not a recognized processor for this target" $t/log
