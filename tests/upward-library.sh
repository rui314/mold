#!/bin/bash
source "$(dirname "$0")"/common.inc

# An upward dylib depends on the one being linked in turn, so dyld need
# not initialize it first (LC_LOAD_UPWARD_DYLIB). Only a dylib can have
# one: ld-prime links the library as usual for anything else, with a
# warning. -upward-l looks for a dylib only.
cat <<EOF | $CC -o $t/foo.o -c -xc -
int foo(void) { return 3; }
EOF
mkdir -p $t/fw/Foo.framework
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,/u/libfoo.dylib
$CC -o $t/libbar.dylib -shared $t/foo.o -Wl,-install_name,/u/libbar.dylib
$CC -o $t/fw/Foo.framework/Foo -shared $t/foo.o -Wl,-install_name,/u/Foo
ar rcs $t/libbaz.a $t/foo.o

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void);
int main() { return foo() != 3; }
EOF

$CC --ld-path=$mold -o $t/lib.dylib -shared $t/a.o -L$t -F$t/fw -Wl,-upward-lfoo \
  -Wl,-upward_library,$t/libbar.dylib -Wl,-upward_framework,Foo
otool -l $t/lib.dylib > $t/log
[ "$(grep -A2 LC_LOAD_UPWARD_DYLIB $t/log | grep -o '/u/[a-zA-Z.]*' | tr '\n' ' ')" = \
  '/u/libfoo.dylib /u/libbar.dylib /u/Foo ' ]

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-upward-lfoo 2> $t/log2
grep -q 'ignoring upward dylib option for /u/libfoo.dylib' $t/log2
otool -l $t/exe > $t/log3
not grep -q LC_LOAD_UPWARD_DYLIB $t/log3
grep -A2 'cmd LC_LOAD_DYLIB' $t/log3 | grep -q /u/libfoo.dylib

not $CC --ld-path=$mold -o $t/lib2.dylib -shared $t/a.o -L$t -Wl,-upward-lbaz 2> $t/log4
grep -q "library 'baz' not found" $t/log4

# A weak upward dylib takes a dylib_use_command, whose flags say both.
$CC --ld-path=$mold -o $t/lib3.dylib -shared $t/a.o -L$t -Wl,-weak-lfoo,-upward-lfoo
otool -l $t/lib3.dylib | grep -A4 'cmd LC_LOAD_WEAK_DYLIB' > $t/log5
grep -q 'name /u/libfoo.dylib (offset 28)' $t/log5
grep -q 'options weak upward' $t/log5

# ld-prime loads a dylib lazily, at its first use, only from macOS 27;
# before, -lazy-l, -lazy_library and -lazy_framework link as usual.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -L$t -F$t/fw -Wl,-lazy-lfoo,-lazy-lfoo \
  -Wl,-lazy_library,$t/libbar.dylib -Wl,-lazy_framework,Foo -mmacosx-version-min=14.0 \
  -Wl,-no_warn_duplicate_libraries 2> $t/log6
[ "$(grep -c "lazy-load will be ignored for 'foo' because deployment target version is too low" $t/log6)" = 1 ]
grep -q "lazy-load will be ignored for '$t/libbar.dylib' because" $t/log6
grep -q "lazy-load will be ignored for 'Foo' because" $t/log6
otool -L $t/exe2 | grep -q /u/libfoo.dylib

# Their load commands follow those of the other libraries the command
# line names (libSystem's too), in naming order.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -L$t -F$t/fw -Wl,-lazy-lfoo \
  -Wl,-lazy_library,$t/libbar.dylib -Wl,-framework,Foo -mmacosx-version-min=14.0 2> /dev/null
otool -L $t/exe3 | awk 'NR > 1 { print $1 }' | tr '\n' ' ' > $t/order3
[ "$(cat $t/order3)" = '/u/Foo /usr/lib/libSystem.B.dylib /u/libfoo.dylib /u/libbar.dylib ' ]

# Firmware - and a -preload image, macOS 27's too - has no dyld to load
# one lazily: each links as usual, with the warning.
$mold -arch $ARCH -preload -e _main -platform_version macos 27.0 27.0 -o $t/exe7 $t/a.o \
  -L$t -F$t/fw -lazy-lbaz -lazy_library $t/libbar.dylib -lazy_framework Foo 2> $t/log7
[ "$(grep -c 'lazy-load will be ignored' $t/log7)" = 3 ]
