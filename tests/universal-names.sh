#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime names a fat file's slice by the file's own path: in -map as
# given ("libfat.a(f.o)"), and in diagnostics by its real path, where
# a fat archive's member adds the slice's architecture before its
# place in the archive: "/abs/libfat.a[arm64][2](f.o)".
echo 'int foo(void) { return 1; } int dup(void) { return 0; }' | $CC -o $t/f.o -c -xc -
rm -f $t/lib.a
ar rcs $t/lib.a $t/f.o
lipo $t/lib.a -create -output $t/libfat.a
lipo $t/f.o -create -output $t/fat.o

echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/m.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/m.o $t/libfat.a -Wl,-map,$t/map
grep -q "\] $t/libfat.a(f.o)\$" $t/map

echo 'int dup(void) { return 2; } int main() { return 0; }' | $CC -o $t/d.o -c -xc -
not $CC --ld-path=$mold -o $t/exe2 $t/d.o -Wl,-force_load,$t/libfat.a 2> $t/log
grep -q "^    /.*/libfat.a\[$ARCH\]\[2\](f.o)\$" $t/log

not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/fat.o 2> $t/log2
grep -q "^    /.*/fat.o\$" $t/log2
