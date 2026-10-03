#!/bin/bash
source "$(dirname "$0")"/common.inc

# -arch_variant names a variant of an architecture (arm64e.x1, of
# arm64e's pointer authentication ABI), which ld-prime takes for some
# arm64e links only: an arm64 or x86_64 link refuses the option,
# whatever it names.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

not $mold -arch $ARCH -o $t/exe $t/a.o -arch_variant arm64e.x1 2> $t/log
grep -q -- "-arch_variant is not supported with -arch $ARCH" $t/log

not $mold -arch $ARCH -o $t/exe $t/a.o -arch_variant x86_64h -r 2> $t/log
grep -q -- "-arch_variant is not supported with -arch $ARCH" $t/log

not $mold -arch_variant bogus -arch $ARCH -o $t/exe $t/a.o 2> $t/log
grep -q -- -arch $t/log

not $mold -arch $ARCH -o $t/exe $t/a.o -arch_variant 2> $t/log
grep -q -- '-arch_variant.*missing' $t/log
