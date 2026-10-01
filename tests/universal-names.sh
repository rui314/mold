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

# The undefined-symbol report names the file a symbol is referenced
# from by its leaf name, an archive member's with its place in the
# archive, a fat archive's with the slice's architecture too. (mold
# names it on the symbol's line, ld-prime on the next.)
echo 'int missing(void); int bar(void) { return missing(); }' | $CC -o $t/u.o -c -xc -
rm -f $t/libu.a
ar rcs $t/libu.a $t/f.o $t/u.o
lipo $t/libu.a -create -output $t/libufat.a
lipo $t/u.o -create -output $t/ufat.o
echo 'int main() { return 0; }' | $CC -o $t/n.o -c -xc -
not $CC --ld-path=$mold -o $t/exe4 $t/n.o $t/ufat.o 2> $t/log4
grep -v '^+' $t/log4 | grep -A1 _missing | grep -Eq '(: | in )ufat\.o'
not grep -q '(for architecture' $t/log4
not $CC --ld-path=$mold -o $t/exe5 $t/n.o -Wl,-force_load,$t/libufat.a 2> $t/log5
grep -v '^+' $t/log5 | grep -A1 _missing | grep -Fq "libufat.a[$ARCH][3](u.o)"
not $CC --ld-path=$mold -o $t/exe6 $t/n.o -Wl,-force_load,$t/libu.a 2> $t/log6
grep -v '^+' $t/log6 | grep -A1 _missing | grep -Eq '(: | in )libu\.a\[3\]\(u\.o\)'

# -why_live names a fat dylib by its path alone.
$CC -shared -o $t/libd.dylib $t/f.o
lipo $t/libd.dylib -create -output $t/libdfat.dylib
$CC --ld-path=$mold -o $t/exe7 $t/m.o $t/libdfat.dylib -Wl,-dead_strip,-why_live,_foo > $t/log7 2>&1
grep -q "^_foo from /.*/libdfat.dylib\$" $t/log7
