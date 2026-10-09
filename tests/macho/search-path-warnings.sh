#!/bin/bash
source "$(dirname "$0")"/common.inc

# A search directory that is no directory is left out with a warning,
# under a syslibroot (the directory itself is then tried, if the
# command line gave it) or not, and so is a directory the command line
# gave that is not there; a default directory that is not there goes
# without a word. -w hides the warnings.
abs=$(cd $t && pwd -P)
root=$t/root
mkdir -p $t/lib $root/usr/lib $root$abs
touch $t/file $root$abs/file2 $root/usr/lib/swift

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

link() {
  $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -o $t/exe $t/a.o \
    $SDK/usr/lib/libSystem.tbd -syslibroot $root "$@" 2>&1 > /dev/null |
    sed '/built for newer/d'
}

link -v -L$t/lib -L$t/nonexist -L$t/file -F$t/nonexist2 -L$abs/file2 > $t/log
grep -q "warning: search path '$t/nonexist' not found" $t/log
grep -q "warning: search path '$t/file' is not a directory" $t/log
grep -q "warning: search path '$t/nonexist2' not found" $t/log
grep -q "warning: -syslibroot and combined search path '$root$abs/file2' is not a directory" $t/log
grep -q "warning: search path '$abs/file2' not found" $t/log
grep -q "warning: -syslibroot and combined search path '$root/usr/lib/swift' is not a directory" $t/log
[ "$(grep -c warning: $t/log)" -eq 6 ]

tab=$(printf '\t')
grep -E "^(Library search paths:|Framework search paths:|$tab)" $t/log > $t/paths
printf 'Library search paths:\n\t%s\n\t%s\nFramework search paths:\n' $t/lib $root/usr/lib |
  diff - $t/paths

link -L$t/nonexist -L$t/file -w > $t/log2
not grep -q warning: $t/log2

# clang gives every link -L/usr/local/lib, and ld-prime says nothing
# when that is not there, as -L or -F, spelled just so; sandbox-exec
# hides it.
hide='(version 1)(allow default)(deny file-read* (subpath "/usr/local/lib"))'
sandbox-exec -p "$hide" $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -o $t/exe \
  $t/a.o $SDK/usr/lib/libSystem.tbd -syslibroot $root -L/usr/local/lib -F/usr/local/lib \
  -L/usr/local/lib/ 2> $t/log3
not grep -q "'/usr/local/lib'" $t/log3
grep -q "warning: search path '/usr/local/lib/' not found" $t/log3
