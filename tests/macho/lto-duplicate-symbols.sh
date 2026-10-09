#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'int dup(void); int main(void) { return dup(); }' | $CC -flto -c -xc - -o $t/main.o
echo 'int dup(void) { return 1; }' | $CC -flto -c -xc - -o $t/bc1.o
echo 'int dup(void) { return 2; }' | $CC -flto -c -xc - -o $t/bc2.o
echo 'int dup(void) { return 3; }' | $CC -c -xc - -o $t/n1.o
dir="$t/"

# The two files each error names (the losing definition's, then the
# winner's), one per line, sorted.
dup_files() {
  sed -n 's/^mold: error: duplicate symbol: \(.*\): _dup$/\1/p' $1 |
    awk -F': ' '{ print $1; print $2 }' | sed "s|$dir||" | sort
}

# A symbol bitcode and a Mach-O object both define shows up once LTO
# has compiled the bitcode, in the object LTO made, which keeps the
# definition. (ld-prime lists the bitcode file too.)
not $CC --ld-path=$mold -flto -o $t/exe1 $t/main.o $t/bc1.o $t/n1.o 2> $t/log1
dup_files $t/log1 > $t/files1
printf '/tmp/lto.o\nn1.o\n' | diff - $t/files1

# One between two bitcode files fails the link before LTO: there is no
# merging their modules. (ld-prime reports it once LTO is done.)
not $CC --ld-path=$mold -flto -o $t/exe2 $t/main.o $t/bc1.o $t/bc2.o 2> $t/log2
dup_files $t/log2 > $t/files2
printf 'bc1.o\nbc2.o\n' | diff - $t/files2
not grep -q -e /tmp/lto.o -e lto_codegen $t/log2

# The object LTO made goes by the -object_path_lto path as given, which
# ld-prime doesn't resolve as it does input files' paths.
not $CC --ld-path=$mold -flto -o $t/exe3 $t/main.o $t/bc1.o $t/n1.o \
  -Wl,-object_path_lto,$t/../$(basename $t)/lto.o 2> $t/log3
grep -qF "$t/../$(basename $t)/lto.o" $t/log3

# So does one between ThinLTO bitcode and other bitcode, which libLTO
# compiles apart. (ld-prime compiles them, and lists the merged
# modules' object too.)
echo 'int dup(void) { return 4; }' | $CC -flto=thin -c -xc - -o $t/bc3.o
not $CC --ld-path=$mold -flto -o $t/exe4 $t/main.o $t/bc3.o $t/bc1.o 2> $t/log4
dup_files $t/log4 > $t/files4
printf 'bc1.o\nbc3.o\n' | diff - $t/files4
not grep -q -e /tmp/lto.o -e lto_codegen $t/log4
