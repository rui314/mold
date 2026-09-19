#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <CoreFoundation/CoreFoundation.h>
int main() { return CFStringGetLength(CFSTR("ab")) != 2; }
EOF2

$CC --ld-path=$mold -weak_framework CoreFoundation -o $t/exe $t/a.o
$t/exe
otool -l $t/exe | grep -q LC_LOAD_WEAK_DYLIB
