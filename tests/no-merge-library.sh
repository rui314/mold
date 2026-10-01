#!/bin/bash
source "$(dirname "$0")"/common.inc

# Xcode links a mergeable library (MERGEABLE_LIBRARY) into a debug build
# with -no_merge-l, -no_merge_framework or -no_merge_library, where a
# release build merges it (-merge_*): the image re-exports the library,
# exactly as -reexport-l, -reexport_framework and -reexport_library
# would have it. ld-prime also adds a hook to the image for the
# library's classes, unless -no_merged_libraries_hook.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 3; }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int foo(void);
int main() { printf("%d\n", foo()); }
EOF

mkdir -p $t/lib $t/Frameworks/Foo.framework $t/x $t/y
$CC --ld-path=$mold -shared -o $t/lib/libfoo.dylib $t/a.o \
  -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -shared -o $t/Frameworks/Foo.framework/Foo $t/a.o \
  -Wl,-install_name,@rpath/Foo.framework/Foo

for opts in "-L$t/lib -Wl,-no_merge-lfoo|-L$t/lib -Wl,-reexport-lfoo" \
  "-Wl,-no_merge_library,$t/lib/libfoo.dylib|-Wl,-reexport_library,$t/lib/libfoo.dylib" \
  "-F$t/Frameworks -Wl,-no_merge_framework,Foo|-F$t/Frameworks -Wl,-reexport_framework,Foo"; do
  $CC --ld-path=$mold -o $t/x/exe $t/main.o ${opts%|*} -Wl,-no_merged_libraries_hook \
    -Wl,-rpath,@loader_path/../lib -Wl,-rpath,@loader_path/../Frameworks
  $CC --ld-path=$mold -o $t/y/exe $t/main.o ${opts#*|} \
    -Wl,-rpath,@loader_path/../lib -Wl,-rpath,@loader_path/../Frameworks
  cmp $t/x/exe $t/y/exe
  otool -L $t/x/exe | grep -q 'foo.dylib .*reexport)\|Foo .*reexport)'
  [ "$($t/x/exe)" = 3 ]
done

# They are spelled as given when repeated, and as the -reexport ones
# when at odds with another naming of the library.
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lfoo \
  -Wl,-no_merge-lfoo -Wl,-no_merged_libraries_hook 2> $t/log1
grep -q "warning: ignoring duplicate libraries: '-no_merge-lfoo'" $t/log1
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-weak-lfoo \
  -Wl,-no_merge-lfoo -Wl,-no_merged_libraries_hook 2> $t/log2
grep -q "'-weak-lfoo' and '-reexport-lfoo' cannot be used together" $t/log2

# Each wants its argument; -no_merge-l looks for a dylib only.
for opt in -no_merge_framework -no_merge_library -no_merge-l; do
  not $mold -arch $ARCH -o $t/exe $t/main.o $opt 2> $t/log3
  grep -q -- "$opt missing <path>" $t/log3
done
ar rcs $t/lib/libbar.a $t/a.o
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lbar \
  -Wl,-no_merged_libraries_hook 2> $t/log4
grep -q "library 'bar' not found" $t/log4

# ld-prime's hook for the classes of such a library (bundleForClassHook.o,
# linked from within ld-prime) is a hook mold doesn't have: it refuses
# to leave it out unasked, for any library, while ld-prime adds it only
# for one with classes.
if $mold -v 2>&1 | grep -q mold-macho; then
  not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lfoo 2> $t/log5
  grep -q -- '-no_merge-lfoo: the hook for the classes of mergeable libraries is not supported; use -no_merged_libraries_hook' $t/log5
else
  $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lfoo
  nm $t/exe > $t/syms5
  not grep -q imageNameHook $t/syms5
  cat <<EOF | $CC -o $t/c.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Bar : NSObject
@end
@implementation Bar
@end
EOF
  $CC --ld-path=$mold -shared -o $t/lib/libc.dylib $t/c.o -framework Foundation
  $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lfoo -Wl,-no_merge-lc
  nm $t/exe > $t/syms6
  grep -q imageNameHook $t/syms6
  grep -q _relinkableLibraryClasses $t/syms6
fi
