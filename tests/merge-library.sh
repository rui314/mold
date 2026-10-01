#!/bin/bash
source "$(dirname "$0")"/common.inc

# -merge-l, -merge_framework and -merge_library link a mergeable dylib
# into the image in place of a load command: one -make_mergeable made,
# which carries its atoms (LC_ATOM_INFO) for the purpose. ld-prime
# refuses any other dylib, also in an image that would ignore it.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 3; }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int foo(void);
int main() { printf("%d\n", foo()); }
EOF

mkdir -p $t/lib $t/Frameworks/Foo.framework
$CC --ld-path=$mold -shared -o $t/lib/libfoo.dylib $t/a.o \
  -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -shared -o $t/Frameworks/Foo.framework/Foo $t/a.o \
  -Wl,-install_name,@rpath/Foo.framework/Foo

not $CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-merge_library,$t/lib/libfoo.dylib 2> $t/log1
grep -q "dylib cannot be merged, not built with -make_mergeable in '$t/lib/libfoo.dylib'" $t/log1
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-merge-lfoo 2> $t/log2
grep -q "dylib cannot be merged, not built with -make_mergeable in '$t/lib/libfoo.dylib'" $t/log2
not $CC --ld-path=$mold -o $t/exe $t/main.o -F$t/Frameworks -Wl,-merge_framework,Foo 2> $t/log3
grep -q "dylib cannot be merged, not built with -make_mergeable in '$t/Frameworks/Foo.framework/Foo'" $t/log3
not $CC --ld-path=$mold -r -o $t/r.o $t/main.o -Wl,-merge_library,$t/lib/libfoo.dylib 2> $t/log4
grep -q "dylib cannot be merged" $t/log4

# A search finds a dylib only, never a stub or an archive.
mkdir -p $t/lib2 $t/Frameworks2/Foo.framework
ar rcs $t/lib2/libfoo.a $t/a.o
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib2 -Wl,-merge-lfoo 2> $t/log5
grep -q "library 'foo' not found" $t/log5
cat > $t/lib/libfoo.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '@rpath/libfoo.dylib'
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo ]
...
EOF
cp $t/lib/libfoo.tbd $t/Frameworks/Foo.framework/Foo.tbd
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-merge-lfoo 2> $t/log6
grep -q "in '$t/lib/libfoo.dylib'" $t/log6
not $CC --ld-path=$mold -o $t/exe $t/main.o -F$t/Frameworks -Wl,-merge_framework,Foo 2> $t/log7
grep -q "in '$t/Frameworks/Foo.framework/Foo'" $t/log7
cp $t/lib/libfoo.tbd $t/Frameworks2/Foo.framework/Foo.tbd
not $CC --ld-path=$mold -o $t/exe $t/main.o -F$t/Frameworks2 -Wl,-merge_framework,Foo 2> $t/log8
grep -q "framework 'Foo' not found" $t/log8
rm $t/lib/libfoo.tbd $t/Frameworks/Foo.framework/Foo.tbd

# An object or an archive the path names links as ever.
mkdir -p $t/x $t/y
$CC --ld-path=$mold -o $t/x/exe $t/main.o -Wl,-merge_library,$t/a.o
$CC --ld-path=$mold -o $t/y/exe $t/main.o $t/a.o
cmp $t/x/exe $t/y/exe
$CC --ld-path=$mold -o $t/x/exe $t/main.o -Wl,-merge_library,$t/lib2/libfoo.a
$CC --ld-path=$mold -o $t/y/exe $t/main.o $t/lib2/libfoo.a
cmp $t/x/exe $t/y/exe

# A merged library has no load command to make weak, upward, lazy or
# re-exported.
for opt in -weak-lfoo -upward-lfoo -reexport-lfoo -no_merge-lfoo; do
  not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-merge-lfoo -Wl,$opt 2> $t/log9
  grep -q "'${opt/no_merge/reexport}' and '-merge-lfoo' cannot be used together" $t/log9
done
not $CC --ld-path=$mold -o $t/exe $t/main.o -F$t/Frameworks -Wl,-upward_framework,Foo \
  -Wl,-merge_framework,Foo 2> $t/log10
grep -q "'-upward_framework Foo' and '-merge_framework Foo' cannot be used together" $t/log10
not $CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-merge_library,$t/lib/libfoo.dylib \
  -Wl,-weak_library,$t/lib/libfoo.dylib 2> $t/log11
grep -q "'-weak-l$t/lib/libfoo.dylib' and '-merge-l$t/lib/libfoo.dylib' cannot be used together" $t/log11

# Repeated, they are spelled as given; each wants its argument.
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-merge-lfoo -Wl,-merge-lfoo 2> $t/log12
grep -q "warning: ignoring duplicate libraries: '-merge-lfoo'" $t/log12
for opt in -merge_framework -merge_library -merge-l; do
  not $mold -arch $ARCH -o $t/exe $t/main.o $opt 2> $t/log13
  grep -q -- "$opt missing <path>" $t/log13
done

# A dylib ld-prime made mergeable is merged into the image, which gets
# no load command for it; an image that links no dylib ignores it as
# it would any dylib.
$CC -shared -o $t/lib/libbar.dylib $t/a.o -Wl,-install_name,@rpath/libbar.dylib \
  -Wl,-make_mergeable
otool -l $t/lib/libbar.dylib | grep -q LC_ATOM_INFO
$CC --ld-path=$mold -r -o $t/r.o $t/main.o -Wl,-merge_library,$t/lib/libbar.dylib 2> $t/log14
grep -q "ignoring unexpected dylib" $t/log14
$CC --ld-path=$mold -o $t/exe $t/main.o -Wl,-merge_library,$t/lib/libbar.dylib
otool -L $t/exe > $t/libs
not grep -q libbar $t/libs
[ "$($t/exe)" = 3 ]
