#!/bin/bash
source "$(dirname "$0")"/common.inc

# -framework Name,suffix links the variant of the framework's binary
# its Name symlink points to, with the suffix appended, if any directory
# has one; -image_suffix has -l and -framework look for each suffixed
# variant (libfoo_debug.dylib), of any extension, before the plain one,
# directory by directory.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int foo();
int main() { return foo(); }
EOF

mkdir -p $t/lib $t/lib2 $t/F/Foo.framework/Versions/A
$CC --ld-path=$mold -o $t/lib/libfoo.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/libfoo.dylib
$CC --ld-path=$mold -o $t/lib2/libfoo_debug.dylib -shared $t/a.o \
  -Wl,-install_name,/usr/lib/libfoo_debug.dylib
cp $t/lib2/libfoo_debug.dylib $t/lib/libfoo_profile.dylib
for v in '' _debug _profile; do
  $CC --ld-path=$mold -o $t/F/Foo.framework/Versions/A/Foo$v -shared $t/a.o \
    -Wl,-install_name,/Library/Frameworks/Foo.framework/Versions/A/Foo$v
done
ln -sf Versions/A/Foo $t/F/Foo.framework/Foo

dylibs() { otool -L $1 | awk 'NR > 1 && !/libSystem/ { print $1 }'; }

$CC --ld-path=$mold -o $t/exe1 $t/b.o -L$t/lib -L$t/lib2 -lfoo -Wl,-image_suffix,_debug
[ "$(dylibs $t/exe1)" = /usr/lib/libfoo.dylib ]
$CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t/lib2 -L$t/lib -lfoo -Wl,-image_suffix,_debug
[ "$(dylibs $t/exe2)" = /usr/lib/libfoo_debug.dylib ]
$CC --ld-path=$mold -o $t/exe3 $t/b.o -L$t/lib -lfoo -Wl,-image_suffix,_debug \
  -Wl,-image_suffix,_profile
[ "$(dylibs $t/exe3)" = /usr/lib/libfoo_debug.dylib ]

$CC --ld-path=$mold -o $t/exe4 $t/b.o -F$t/F -framework Foo,_debug
[ "$(dylibs $t/exe4)" = /Library/Frameworks/Foo.framework/Versions/A/Foo_debug ]
$CC --ld-path=$mold -o $t/exe5 $t/b.o -F$t/F -framework Foo,_nope
[ "$(dylibs $t/exe5)" = /Library/Frameworks/Foo.framework/Versions/A/Foo ]
$CC --ld-path=$mold -o $t/exe6 $t/b.o -F$t/F -framework Foo,_profile -Wl,-image_suffix,_debug
[ "$(dylibs $t/exe6)" = /Library/Frameworks/Foo.framework/Versions/A/Foo_profile ]
not $CC --ld-path=$mold -o $t/exe7 $t/b.o -F$t/F -framework Nope,_debug 2> $t/log
grep -q "framework 'Nope,_debug' not found" $t/log

not $mold -o $t/exe8 $t/b.o -image_suffix 2> $t/log
grep -q -- '-image_suffix missing <suffix>' $t/log
