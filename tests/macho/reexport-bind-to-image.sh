#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image that re-exports two or more libraries from locations that
# aren't public (as an umbrella framework does its sub-frameworks, or a
# debug build its mergeable libraries with -no_merge_*) binds the
# imports from them to itself, ordinal 0, as ld64 and ld-prime do: dyld
# finds them through the image's re-exports. Their load commands stay
# as they are.
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int foo(void); int bar(void); int baz(void);
int main() { printf("%d\n", foo() + bar() + baz()); }
EOF
mkdir -p $t/lib
for f in foo bar baz; do
  echo "int $f(void) { return 1; }" | $CC -o $t/$f.o -c -xc -
  $CC --ld-path=$mold -shared -o $t/lib/lib$f.dylib $t/$f.o \
    -Wl,-install_name,@rpath/lib$f.dylib
done
# The same at a public location (not public under -no_implicit_dylibs).
$CC --ld-path=$mold -shared -o $t/lib/libpfoo.dylib $t/foo.o \
  -Wl,-install_name,/usr/lib/libpfoo.dylib
$CC --ld-path=$mold -shared -o $t/lib/libpbar.dylib $t/bar.o \
  -Wl,-install_name,/usr/lib/libpbar.dylib

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-reexport-lfoo \
  -Wl,-reexport-lbar -lbaz -Wl,-rpath,@loader_path/lib
dyld_info -fixups $t/exe > $t/fixups1
grep __got $t/fixups1 | grep -v libSystem > $t/got1
[ "$(awk '{print $NF}' $t/got1 | sort | tr '\n' ' ')" = "<this-image>/_bar <this-image>/_foo libbaz/_baz " ]
otool -L $t/exe > $t/libs1
grep -A1 'libfoo.dylib .*reexport)' $t/libs1 | grep -q 'libbar.dylib .*reexport)'
[ "$($RUN $t/exe)" = 3 ]

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lfoo \
  -Wl,-no_merge-lbar -lbaz -Wl,-rpath,@loader_path/lib
dyld_info -fixups $t/exe > $t/fixups2
grep -q 'bind *<this-image>/_foo' $t/fixups2
[ "$($RUN $t/exe)" = 3 ]

# Not with one such library alone.
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-reexport-lfoo \
  -lbar -lbaz -Wl,-rpath,@loader_path/lib
dyld_info -fixups $t/exe > $t/fixups3
grep -q 'bind *libfoo/_foo' $t/fixups3
not grep -q this-image $t/fixups3
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-reexport-lfoo \
  -Wl,-reexport-lpbar -lbaz
dyld_info -fixups $t/exe > $t/fixups4
grep -q 'bind *libfoo/_foo' $t/fixups4
grep -q 'bind *libpbar/_bar' $t/fixups4
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-reexport-lpfoo \
  -Wl,-reexport-lpbar -lbaz
dyld_info -fixups $t/exe > $t/fixups5
not grep -q this-image $t/fixups5
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-reexport-lpfoo \
  -Wl,-reexport-lpbar -lbaz -Wl,-no_implicit_dylibs
dyld_info -fixups $t/exe > $t/fixups6
grep -q 'bind *<this-image>/_foo' $t/fixups6
grep -q 'bind *<this-image>/_bar' $t/fixups6
