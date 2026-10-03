#!/bin/bash
source "$(dirname "$0")"/common.inc

# An input built for a newer OS than the output targets draws a
# warning. A -r link without -platform_version takes the first object's
# deployment target for its output, and ld-prime warns about the later
# inputs built for a newer one just the same.
echo 'int f14(void) { return 1; }' | $CC -o $t/v14.o -c -xc - -mmacosx-version-min=14.0
echo 'int f15(void) { return 2; }' | $CC -o $t/v15.o -c -xc - -mmacosx-version-min=15.0

$mold -arch $ARCH -r $t/v14.o $t/v15.o -o $t/r1.o 2> $t/log1
grep -q "(.*v15.o) was built for newer 'macOS' version (15.0) than being linked (14.0)" $t/log1
otool -l $t/r1.o > $t/lc1
grep -A4 'cmd LC_BUILD_VERSION' $t/lc1 > $t/bv1
grep -q 'minos 14.0' $t/bv1

$mold -arch $ARCH -r $t/v15.o $t/v14.o -o $t/r2.o 2> $t/log2
not grep -q 'built for newer' $t/log2

echo 'int main() { return 0; }' | $CC -o $t/m.o -c -xc - -mmacosx-version-min=14.0
$CC --ld-path=$mold -o $t/exe $t/m.o $t/v15.o -mmacosx-version-min=14.0 2> $t/log3
grep -q "(.*v15.o) was built for newer 'macOS' version (15.0) than being linked (14.0)" $t/log3

# The file goes by its path, an archive member as archive(member).
# (ld-prime names the file by its real path, and an archive member by
# the archive's real path and the member's position among its entries.)
mkdir -p $t/sub
cp $t/v15.o $t/sub/
$CC --ld-path=$mold -o $t/exe2 $t/m.o $t/sub/../sub/v15.o -mmacosx-version-min=14.0 2> $t/log4
grep -q "object file ($t/sub/../sub/v15.o) was built for newer" $t/log4
rm -f $t/lib15.a
ar rcs $t/lib15.a $t/v14.o $t/v15.o
echo 'int f15(void); int main() { return f15(); }' | $CC -o $t/m2.o -c -xc - -mmacosx-version-min=14.0
$CC --ld-path=$mold -o $t/exe3 $t/m2.o $t/lib15.a -mmacosx-version-min=14.0 2> $t/log5
grep -q "object file ($t/lib15.a(v15.o)) was built for newer" $t/log5
