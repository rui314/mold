#!/bin/bash
source "$(dirname "$0")"/common.inc

# Only the files the link takes count in whether an import is weak: a
# strong reference in an archive member the link doesn't load leaves an
# import every loaded reference to which is weak a weak import, and its
# dylib a weak load, as ld-prime does. (Kickstarter's test bundle weakly
# imports StoreKit's SKAdNetwork; a GoogleAppMeasurement member left out
# references it strongly.)
echo 'int wf(void) { return 1; }' | $CC -o $t/lib.o -c -xc -
$CC --ld-path=$mold -dynamiclib -o $t/libwl.dylib $t/lib.o -install_name @rpath/libwl.dylib
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern int wf(void) __attribute__((weak_import));
int main() { printf("%d\n", wf ? wf() : 0); return 0; }
EOF
cat <<EOF | $CC -o $t/strong.o -c -xc -
int wf(void);
int unused(void) { return wf(); }
EOF
rm -f $t/lib.a
ar rcs $t/lib.a $t/strong.o

$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a $t/libwl.dylib -Wl,-rpath,$PWD/$t
nm -m $t/exe > $t/nm
not grep -q _unused $t/nm
grep -q 'weak external _wf' $t/nm
otool -L $t/exe | grep libwl.dylib | grep -q 'weak)'
$RUN $t/exe | grep -q '^1$'

# Loaded, the member's reference makes the import strong.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/strong.o $t/libwl.dylib -Wl,-rpath,$PWD/$t
otool -L $t/exe2 | grep libwl.dylib > $t/load
not grep -q 'weak)' $t/load
