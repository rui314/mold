#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <CoreFoundation/CoreFoundation.h>
int main() { return CFStringGetLength(CFSTR("ab")) != 2; }
EOF2

$CC --ld-path=$mold -weak_framework CoreFoundation -o $t/exe $t/a.o
$t/exe
otool -l $t/exe | grep LC_LOAD_WEAK_DYLIB
# Every import from a weak-linked framework is a weak import.
dyld_info -fixups $t/exe > $t/fixups
grep 'CoreFoundation/_CFStringGetLength \[weak-import\]' $t/fixups
nm -m $t/exe > $t/nm
grep -q 'undefined) weak external _CFStringGetLength' $t/nm
grep -q 'undefined) weak external ___CFConstantStringClassReference' $t/nm
