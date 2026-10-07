#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores the options that asked for a bitcode bundle, and
# -ld_classic, which once picked ld64 over it, with a warning; -ld_new
# picks ld-prime, which -ld_prime does too, with another. None takes an
# argument.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc - -mmacosx-version-min=14.0
link() {
  $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$SDK" -lSystem $t/a.o \
    -o $t/exe "$@"
}

link
cp $t/exe $t/exe0

for opt in -bitcode_bundle -bitcode_hide_symbols -bitcode_process_mode -bitcode_symbol_map \
  -bitcode_verify -ld_classic; do
  link $opt 2> $t/log
  grep -q -- "warning: $opt is no longer supported and will be ignored$" $t/log
  cmp $t/exe $t/exe0
done

link -ld_prime 2> $t/log
grep -q -- 'warning: -ld_prime is deprecated, use -ld_new instead$' $t/log
cmp $t/exe $t/exe0

link -ld_new 2> $t/log
not grep -q warning $t/log
cmp $t/exe $t/exe0

link -bitcode_bundle -X -ld_classic -ld_prime 2> $t/log
[ "$(grep -c warning $t/log)" = 4 ]
cmp $t/exe $t/exe0

link -w -bitcode_verify -ld_classic 2> $t/log
not grep -q warning $t/log
