#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores the options that asked for a bitcode bundle, and
# -ld_classic, which once picked ld64 over it, with a warning; -ld_new
# picks ld-prime, which -ld_prime does too, with another. None takes an
# argument. The bitcode and -ld_prime warnings come once every option
# is read, with the obsolete options', -ld_classic's as it is read.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc - -mmacosx-version-min=14.0
sdk=$(xcrun --show-sdk-path)
link() {
  $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" -lSystem $t/a.o \
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
[ "$(grep warning $t/log | sed 's/^[a-z]*: warning: //' | tr '\n' '|')" = \
  "-ld_classic is no longer supported and will be ignored|-bitcode_bundle is no longer supported and will be ignored|-X is obsolete|-ld_prime is deprecated, use -ld_new instead|" ]

link -bitcode_verify -ld_classic -ld_prime -w 2> $t/log
grep warning $t/log > $t/log2
grep -q -- '-ld_classic' $t/log2
not grep -q -- '-bitcode_verify\|-ld_prime' $t/log2
