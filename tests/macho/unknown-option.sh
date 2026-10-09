#!/bin/bash
source "$(dirname "$0")"/common.inc

# An option the linker doesn't know fails the link, naming the option.
echo 'int main() {}' | $CC -c -xc - -o $t/a.o

not $mold -arch $ARCH -o $t/exe $t/a.o -foo 2> $t/log
grep -q -- -foo $t/log

not $mold -arch $ARCH -o $t/exe $t/a.o -sectalign __TEXT __text 4 -bar 2> $t/log
grep -q -- -bar $t/log
