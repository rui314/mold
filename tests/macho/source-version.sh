#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# LC_SOURCE_VERSION came with macOS 10.8: an image for an older macOS,
# on any architecture and of any kind, has none, unless
# -add_source_version asks; -no_source_version leaves it out of any.
# The last of the two wins.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

link() {
  $mold -arch $ARCH -syslibroot $SDK -o $t/$1 $t/a.o -lSystem -e _main \
    -platform_version macos "${@:2}" 2> /dev/null
}
has_cmd() {
  otool -l $t/$1 > $t/$1.lc
  grep -q 'cmd LC_SOURCE_VERSION' $t/$1.lc
}

link exe1 10.7 27.0
not has_cmd exe1
link exe2 10.8 27.0
has_cmd exe2
link exe3 10.7 27.0 -no_source_version -add_source_version
has_cmd exe3
link exe4 13.0 27.0 -add_source_version -no_source_version
not has_cmd exe4
link lib1.dylib 10.7 27.0 -dylib
not has_cmd lib1.dylib
link lib2.dylib 10.8 27.0 -dylib
has_cmd lib2.dylib

# -source_version sets the version, a.b.c.d.e, and asks for the command
# as -add_source_version does. a takes 24 bits, the others 10 each.
link exe5 10.7 27.0 -no_source_version -source_version 1.2.3.4.5
otool -l $t/exe5 | grep -A2 'cmd LC_SOURCE_VERSION' | grep 'version 1.2.3.4.5$'
link exe6 13.0 27.0 -source_version 16777215.0.1023
otool -l $t/exe6 | grep -A2 'cmd LC_SOURCE_VERSION' | grep 'version 16777215.0.1023$'
not link exe7 13.0 27.0 -source_version 1.1024 2> $t/log7
not link exe7 13.0 27.0 -source_version 16777216 2>> $t/log7
not link exe7 13.0 27.0 -source_version 1.2. 2>> $t/log7
not $mold -arch $ARCH -o $t/exe7 $t/a.o -source_version 1.x 2>> $t/log7
grep -q -- '-source_version: malformed 64-bit a.b.c.d.e version number: 1.x$' $t/log7

# The build system's version, $RC_ProjectSourceVersion, stands in for
# -source_version, read unless -no_source_version says there is none;
# a malformed one is 0, with a warning.
RC_ProjectSourceVersion=7.8.9 link exe8 13.0 27.0
otool -l $t/exe8 | grep -A2 'cmd LC_SOURCE_VERSION' | grep 'version 7.8.9$'
RC_ProjectSourceVersion=7.8.9 link exe9 13.0 27.0 -source_version 1
otool -l $t/exe9 | grep -A2 'cmd LC_SOURCE_VERSION' | grep 'version 1.0$'
RC_ProjectSourceVersion=abc $mold -arch $ARCH -syslibroot $SDK -o $t/exe10 $t/a.o \
  -lSystem -platform_version macos 13.0 27.0 2> $t/log10
grep -q 'warning: \$RC_ProjectSourceVersion: malformed 64-bit a.b.c.d.e version number: abc' $t/log10
otool -l $t/exe10 | grep -A2 'cmd LC_SOURCE_VERSION' | grep 'version 0.0$'
RC_ProjectSourceVersion=abc $mold -arch $ARCH -syslibroot $SDK -o $t/exe11 $t/a.o \
  -lSystem -platform_version macos 13.0 27.0 -no_source_version 2> $t/log11
not grep -q RC_ProjectSourceVersion $t/log11
