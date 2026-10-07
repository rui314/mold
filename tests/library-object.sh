#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -l name that ends in .o names a file to look up in the library
# search path as it is, with no lib prefix and no other extension,
# whatever the option; it goes where the option is among the inputs.
# clang names crt1.o -lcrt1.10.6.o for an x86-64 macOS before 10.8.
mkdir -p $t/c $t/d

echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/main.o -c -xc -
echo 'int foo(void) { return 0; }' | $CC -o $t/c/foo.o -c -xc -
ar rcs $t/d/libfoo.o.a $t/c/foo.o

link() {
  $mold -arch $ARCH -platform_version macos 13.0 13.0 $t/main.o \
    $SDK/usr/lib/libSystem.tbd "$@"
}

link -o $t/exe -L$t/d -L$t/c -lfoo.o -map $t/map 2> /dev/null
grep -qx "\[  2\] $t/c/foo.o" $t/map
nm $t/exe | grep -q ' T _foo$'

link -o $t/exe2 -L$t/c -upward-lfoo.o 2> /dev/null
nm $t/exe2 | grep -q ' T _foo$'

not link -o $t/exe3 -L$t/d -lfoo.o 2> $t/log3
grep -q "library 'foo.o' not found" $t/log3

if [ $ARCH = x86_64 ]; then
  echo 'int main() { return 0; }' | $CC -o $t/old.o -c -xc - -mmacosx-version-min=10.7
  $CC --ld-path=$mold -o $t/exe4 $t/old.o -mmacosx-version-min=10.7 2> /dev/null
  nm $t/exe4 | grep -q ' T start$'
fi
