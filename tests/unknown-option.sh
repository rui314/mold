#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime reports the options it doesn't know together, once it has
# read the others: after the warnings they drew, and after none of the
# errors, which come as it reads an option.
echo 'int main() {}' | $CC -c -xc - -o $t/a.o

not $mold -arch $ARCH -o $t/exe $t/a.o -foo -sectalign __TEXT __text 3 -bar 2> $t/log
grep -q 'alignment for -sectalign __TEXT __text is not a power of two' $t/log
grep -q 'unknown options: -foo -bar $' $t/log
[ "$(grep -n 'not a power of two' $t/log | cut -d: -f1)" -lt \
  "$(grep -n 'unknown options' $t/log | cut -d: -f1)" ]

not $mold -arch $ARCH -o $t/exe $t/a.o -foo -image_base zz 2> $t/log2
grep -q -- '-image_base: not a hexadecimal number: zz' $t/log2
not grep -q 'unknown options' $t/log2
