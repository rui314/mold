#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'int dup(void); int main(void) { return dup(); }' | $CC -flto -c -xc - -o $t/main.o
echo 'int dup(void) { return 1; }' | $CC -flto -c -xc - -o $t/bc1.o
echo 'int dup(void) { return 2; }' | $CC -flto -c -xc - -o $t/bc2.o
echo 'int dup(void) { return 3; }' | $CC -c -xc - -o $t/n1.o
dir="$(pwd -P)/$t/"

# A symbol bitcode and a Mach-O object both define shows up once LTO
# has compiled the bitcode: ld-prime lists the object, then the bitcode
# file, then the object LTO made, which keeps the definition.
not $CC --ld-path=$mold -flto -o $t/exe1 $t/main.o $t/bc1.o $t/n1.o 2> $t/log1
grep -A3 "^duplicate symbol '_dup' in:" $t/log1 | sed -e 1d -e "s|$dir||" > $t/files1
printf '    n1.o\n    bc1.o\n    /tmp/lto.o\n' | diff - $t/files1
grep -q ': 1 duplicate symbols$' $t/log1

# One between two bitcode files fails the link before LTO, which could
# not merge their modules (ld-prime lists them in no stable order).
not $CC --ld-path=$mold -flto -o $t/exe2 $t/main.o $t/bc1.o $t/bc2.o 2> $t/log2
grep -A2 "^duplicate symbol '_dup' in:" $t/log2 | sed -e 1d -e "s|$dir||" | sort > $t/files2
printf '    bc1.o\n    bc2.o\n' | diff - $t/files2
grep -q ': 1 duplicate symbols$' $t/log2
not grep -q -e /tmp/lto.o -e lto_codegen $t/log2
