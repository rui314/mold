#!/bin/bash
source "$(dirname "$0")"/common.inc

# LC_SOURCE_VERSION came with macOS 10.8: an image for an older macOS,
# on any architecture and of any kind, has none, unless
# -add_source_version asks; -no_source_version leaves it out of any.
# The last of the two wins.
sdk=$(xcrun --show-sdk-path)
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

link() {
  $mold -arch $ARCH -syslibroot $sdk -o $t/$1 $t/a.o -lSystem -e _main \
    -platform_version macos "${@:2}" 2> /dev/null
}
has_cmd() {
  otool -l $t/$1 > $t/$1.lc
  grep -q 'cmd LC_SOURCE_VERSION' $t/$1.lc
}

link exe1 10.7 27.0
not has_cmd exe1
link exe2 10.8 27.0
has_cmd exe2
link exe3 10.7 27.0 -no_source_version -add_source_version
has_cmd exe3
link exe4 13.0 27.0 -add_source_version -no_source_version
not has_cmd exe4
link lib1.dylib 10.7 27.0 -dylib
not has_cmd lib1.dylib
link lib2.dylib 10.8 27.0 -dylib
has_cmd lib2.dylib
