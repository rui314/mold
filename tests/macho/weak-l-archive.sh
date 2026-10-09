#!/bin/bash
source "$(dirname "$0")"/common.inc

# Only a dylib's imports can be weak. ld-prime links an archive a
# -weak-l finds as usual, with a warning, once for the library however
# many options name it; -weak_library says nothing.
echo 'int bar(void) { return 1; }' | $CC -o $t/bar.o -c -xc -
mkdir -p $t/ar
rm -f $t/ar/libbar.a
ar rcs $t/ar/libbar.a $t/bar.o
echo 'int bar(void); int main() { return bar() - 1; }' | $CC -o $t/a.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t/ar -Wl,-weak-lbar,-lbar 2> $t/log
[ "$(grep -c "\-weak-lbar resolved to a static library '$t/ar/libbar.a', but only dynamic libraries can be weak linked. Use -lbar when linking static libraries, or make sure .dylib/.tbd library is located in -L search paths." $t/log)" = 1 ]
nm $t/exe | grep -q ' T _bar$'

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-weak_library,$t/ar/libbar.a 2> $t/log2
not grep -q 'weak linked' $t/log2
