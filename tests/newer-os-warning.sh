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
