#!/bin/bash
source "$(dirname "$0")"/common.inc

# Xcode links a mergeable library (MERGEABLE_LIBRARY) into a debug build
# with -no_merge-l, -no_merge_framework or -no_merge_library, where a
# release build merges it (-merge_*): the image re-exports the library,
# exactly as -reexport-l, -reexport_framework and -reexport_library
# would have it, as long as the library exports no class (see below).
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
  $CC --ld-path=$mold -o $t/x/exe $t/main.o ${opts%|*} \
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
  -Wl,-no_merge-lfoo 2> $t/log1
grep -q "warning: ignoring duplicate libraries: '.*foo'" $t/log1
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-weak-lfoo \
  -Wl,-no_merge-lfoo 2> $t/log2
grep -q "'-weak-lfoo' and '-reexport-lfoo' cannot be used together" $t/log2

# Each wants its argument; -no_merge-l looks for a dylib only.
for opt in -no_merge_framework -no_merge_library -no_merge-l; do
  not $mold -arch $ARCH -o $t/exe $t/main.o $opt 2> $t/log3
  grep -q -- "$opt.*missing" $t/log3
done
ar rcs $t/lib/libbar.a $t/a.o
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t/lib -Wl,-no_merge-lbar 2> $t/log4
grep -q "library 'bar' not found" $t/log4

# The linker also links its hook for the classes of mergeable libraries
# into the image (see merged-libraries-hook.sh), unless
# -no_merged_libraries_hook, if a library a -no_merge_* option names
# exports a class itself: an Objective-C class or metaclass object, or
# a Swift class's type metadata ($s...CN), as the names go. The hook
# has the Objective-C runtime place each class in the library's
# framework, whose resources stay in the app bundle. Its initializer
# has the same name in mold's and ld-prime's. (The ERR trap doesn't
# reach into a function: each command's status goes to the caller.)
needs_hook() {
  rm -f $t/exe
  $CC --ld-path=$mold -o $t/exe $t/main.o "$@" && nm $t/exe > $t/syms &&
    grep -q __ZL11constructorv $t/syms
}
no_hook() {
  rm -f $t/exe
  $CC --ld-path=$mold -o $t/exe $t/main.o "$@" && nm $t/exe > $t/syms &&
    not grep -q __ZL11constructorv $t/syms
}

cat <<EOF | $CC -o $t/c.o -c -xobjective-c -
#import <Foundation/Foundation.h>
int foo(void) { return 3; }
@interface Bar : NSObject
@end
@implementation Bar
@end
EOF
$CC --ld-path=$mold -shared -o $t/lib/libc.dylib $t/c.o -framework Foundation \
  -Wl,-install_name,@rpath/libc.dylib
needs_hook -L$t/lib -Wl,-no_merge-lc
no_hook -L$t/lib -Wl,-no_merge-lc -Wl,-no_merged_libraries_hook
no_hook -L$t/lib -Wl,-reexport-lc
no_hook -L$t/lib -Wl,-no_merge-lfoo -lc

# Whichever naming of the library loads it.
needs_hook -L$t/lib -lc -Wl,-no_merge_library,./$t/lib/libc.dylib

# Not the classes of a library it re-exports in turn.
$CC --ld-path=$mold -shared -o $t/lib/libumbrella.dylib $t/a.o \
  -Wl,-reexport_library,$t/lib/libc.dylib -Wl,-install_name,@rpath/libumbrella.dylib \
  -Wl,-rpath,$(pwd)/$t/lib
no_hook -L$t/lib -Wl,-no_merge-lumbrella

# Nor a class it doesn't export.
cat <<EOF | $CC -o $t/d.o -c -xobjective-c -
#import <Foundation/Foundation.h>
int foo(void) { return 3; }
__attribute__((visibility("hidden"))) @interface Baz : NSObject
@end
@implementation Baz
@end
EOF
$CC --ld-path=$mold -shared -o $t/lib/libd.dylib $t/d.o -framework Foundation \
  -Wl,-install_name,@rpath/libd.dylib
no_hook -L$t/lib -Wl,-no_merge-ld

# The names count, whatever they stand for: the type metadata of a
# Swift class (C), not of a struct (V), nor its nominal type descriptor.
for sym in '_OBJC_METACLASS_$_Qux' '_$s3Foo3BarCN' '_$s3Foo3BarVN' '_$s3Foo3BarCMn' \
  '_OBJC_EHTYPE_$_Qux'; do
  printf 'int foo(void) { return 3; }\nint x __asm__("%s") = 1;\n' "$sym" > $t/e.c
  $CC -o $t/e.o -c $t/e.c
  $CC --ld-path=$mold -shared -o $t/lib/libe.dylib $t/e.o -Wl,-install_name,@rpath/libe.dylib
  case "$sym" in
  *CN|_OBJC_METACLASS*) needs_hook -L$t/lib -Wl,-no_merge-le ;;
  *) no_hook -L$t/lib -Wl,-no_merge-le ;;
  esac
done

# A stub's classes count too, but those $ld$hide hides for the target.
cat > $t/lib/libf.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '@rpath/libf.dylib'
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo ]
    objc-classes:    [ Qux ]
...
EOF
needs_hook -L$t/lib -Wl,-no_merge-lf
cat > $t/lib/libg.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '@rpath/libg.dylib'
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo, '\$ld\$hide\$os11.0\$_OBJC_CLASS_\$_Qux',
                       '\$ld\$hide\$os11.0\$_OBJC_METACLASS_\$_Qux' ]
    objc-classes:    [ Qux ]
...
EOF
needs_hook -L$t/lib -Wl,-no_merge-lg -mmacos-version-min=12.0
no_hook -L$t/lib -Wl,-no_merge-lg -mmacos-version-min=11.0

# The hook binds to each class a re-exported library exports, once.
needs_hook -L$t/lib -Wl,-no_merge-lfoo -Wl,-no_merge-lc
dyld_info -fixups $t/exe > $t/fixups
[ "$(grep -c 'bind .*/_OBJC_CLASS_\$_Bar$' $t/fixups)" = 1 ]
