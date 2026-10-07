#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image for a macOS before 10.9 keeps the FDE of a function that has
# a compact unwind record (the record still wins in __unwind_info), as
# ld64 did for the unwinders of those releases. -keep_dwarf_unwind and
# -no_keep_dwarf_unwind are obsolete: ld-prime goes by the target.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc - -femit-dwarf-unwind=always

link() {
  $mold -arch $ARCH -syslibroot $SDK -o $t/$1 $t/a.o -lSystem -e _main \
    -platform_version macos "${@:2}" 2>> $t/log
}
sects() { otool -l $t/$1 | awk '/sectname/ { print $2 }' > $t/$1.sects; }

link exe1 10.9 27.0 -keep_dwarf_unwind
grep -q -- '-keep_dwarf_unwind is obsolete' $t/log
link exe2 10.8 27.0 -no_keep_dwarf_unwind
grep -q -- '-no_keep_dwarf_unwind is obsolete' $t/log

# Clang gives arm64 code compact unwind records alone.
otool -l $t/a.o > $t/lc
grep -q __eh_frame $t/lc || skip

sects exe1
not grep -q __eh_frame $t/exe1.sects
sects exe2
grep -q __eh_frame $t/exe2.sects
link lib.dylib 10.8 27.0 -dylib
sects lib.dylib
grep -q __eh_frame $t/lib.dylib.sects
