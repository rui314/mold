#!/bin/bash
source "$(dirname "$0")"/common.inc

# The test picks the platforms it links for itself.
on_simulator && skip

# Firmware has no dyld to load a library lazily, and neither has a
# -preload image, which ld-prime counts as firmware's: a library
# -lazy-l, -lazy_library or -lazy_framework names links as -l or
# -framework would, with a warning that says so.
cat <<EOF | $CC -o $t/lib.o -c -xc -
int foo(void) { return 3; }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
int foo(void);
int main(void) { return foo(); }
EOF

$mold -arch $ARCH -platform_version macos 14.0 15.0 -syslibroot $SDK -dylib \
  -install_name /usr/lib/libfoo.dylib -o $t/libfoo.dylib $t/lib.o -lSystem
mkdir -p $t/Foo.framework
cp $t/libfoo.dylib $t/Foo.framework/Foo

fw() { $mold -arch $ARCH -platform_version firmware 1.0 1.0 -o $t/exe $t/main.o "$@" 2> $t/log; }

fw -lazy_library $t/libfoo.dylib
grep -q -- '-lazy_library cannot be used on firmware, changing to regular link' $t/log
otool -l $t/exe > $t/lc
grep -q LC_LOAD_DYLIB $t/lc
not grep -q LC_LAZY_LOAD_DYLIB $t/lc

fw -lazy-lfoo -L$t
grep -q -- '-lazy_library cannot be used on firmware' $t/log
fw -lazy_framework Foo -F$t
grep -q -- '-lazy_framework cannot be used on firmware, changing to regular -framework' $t/log

$mold -arch $ARCH -platform_version macos 27.0 27.0 -preload -e _main -o $t/exe $t/main.o \
  $t/lib.o -lazy_library $t/libfoo.dylib 2> $t/log
grep -q -- '-lazy_library cannot be used on firmware' $t/log

$mold -arch $ARCH -platform_version macos 27.0 27.0 -syslibroot $SDK -o $t/exe $t/main.o \
  -lSystem -lazy_library $t/libfoo.dylib 2> $t/log
not grep -q 'cannot be used' $t/log
