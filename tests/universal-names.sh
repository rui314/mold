#!/bin/bash
source "$(dirname "$0")"/common.inc

# A fat file's slice goes by the file's own path in -map
# ("libfat.a(f.o)"), and in diagnostics with the slice's architecture:
# "libfat.a(for architecture arm64)(f.o)". (ld-prime names it in
# diagnostics by its real path, a fat archive's member with the
# architecture and its place in the archive: "/abs/libfat.a[arm64][2](f.o)".)
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
grep -q "^    $t/libfat.a(for architecture $ARCH)(f.o)\$" $t/log

not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/fat.o 2> $t/log2
grep -q "^    $t/fat.o(for architecture $ARCH)\$" $t/log2

# So does the undefined-symbol report. (ld-prime names the file by its
# leaf name, an archive member's with its place in the archive, a fat
# archive's with the slice's architecture too, on the line after the
# symbol's.)
echo 'int missing(void); int bar(void) { return missing(); }' | $CC -o $t/u.o -c -xc -
rm -f $t/libu.a
ar rcs $t/libu.a $t/f.o $t/u.o
lipo $t/libu.a -create -output $t/libufat.a
lipo $t/u.o -create -output $t/ufat.o
echo 'int main() { return 0; }' | $CC -o $t/n.o -c -xc -
not $CC --ld-path=$mold -o $t/exe4 $t/n.o $t/ufat.o 2> $t/log4
grep -v '^+' $t/log4 | grep -Fq "$t/ufat.o(for architecture $ARCH): _missing"
not $CC --ld-path=$mold -o $t/exe5 $t/n.o -Wl,-force_load,$t/libufat.a 2> $t/log5
grep -v '^+' $t/log5 | grep -Fq "$t/libufat.a(for architecture $ARCH)(u.o): _missing"
not $CC --ld-path=$mold -o $t/exe6 $t/n.o -Wl,-force_load,$t/libu.a 2> $t/log6
grep -v '^+' $t/log6 | grep -Fq "$t/libu.a(u.o): _missing"

# -why_live names a fat dylib by its path.
$CC -shared -o $t/libd.dylib $t/f.o
lipo $t/libd.dylib -create -output $t/libdfat.dylib
$CC --ld-path=$mold -o $t/exe7 $t/m.o $t/libdfat.dylib -Wl,-dead_strip,-why_live,_foo > $t/log7 2>&1
grep -q "^_foo from $t/libdfat.dylib" $t/log7
