#!/bin/bash
source "$(dirname "$0")"/common.inc

# An object for another subtype of the link's CPU type, x86_64h in an
# x86_64 link, is one ld-prime ignores with a warning, unless
# -allow_sub_type_mismatches: it links it then, with a warning as it
# reads it and another once the link uses it. arm64e objects, whose
# pointers are signed, stay out of an arm64 link all the same.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
link() { $CC --ld-path=$mold -o $t/exe $t/a.o "$@"; }

if [ $ARCH = arm64 ]; then
  echo 'int foo() { return 1; }' | cc -arch arm64e -o $t/b.o -c -xc -
  link $t/b.o -Wl,-allow_sub_type_mismatches 2> $t/log
  grep -q "ignoring file '$t/b.o': found architecture 'arm64e', required architecture 'arm64'" $t/log
  exit
fi

echo 'int foo() { return 1; }' | cc -arch x86_64h -o $t/b.o -c -xc -
rm -f $t/libb.a
ar rcs $t/libb.a $t/b.o
msg="linking x86_64h file '$t/b.o' into x86_64 link"

link $t/b.o 2> $t/log
grep -q "ignoring file '$t/b.o': found architecture 'x86_64h', required architecture 'x86_64'" $t/log

link $t/b.o -Wl,-allow_sub_type_mismatches 2> $t/log
[ "$(grep -c "warning: $msg" $t/log)" = 2 ]
nm $t/exe | grep -q ' T _foo$'

link $t/libb.a -Wl,-allow_sub_type_mismatches,-u,_foo 2> $t/log
[ "$(grep -c "warning: linking x86_64h file '$t/libb.a(b.o)' into x86_64 link" $t/log)" = 2 ]
nm $t/exe | grep -q ' T _foo$'

link $t/libb.a -Wl,-allow_sub_type_mismatches 2> $t/log
[ "$(grep -c "warning: linking x86_64h file '$t/libb.a(b.o)' into x86_64 link" $t/log)" = 1 ]

# The second warning comes once ld-prime has checked the inputs'
# versions.
echo 'int main() { return 0; }' | $CC -o $t/new.o -c -xc - -mmacosx-version-min=99.0
$CC --ld-path=$mold -o $t/exe $t/new.o $t/b.o -Wl,-allow_sub_type_mismatches 2> $t/log
grep 'warning: ' $t/log | sed -n 3p | grep -q "$msg"
grep 'warning: ' $t/log | sed -n 2p | grep -q 'was built for newer'

# A fat file with no slice for the link's architecture gives up one of
# another subtype the same way, named by the fat file's path.
lipo $t/b.o -create -output $t/fat.o
lipo $t/libb.a -create -output $t/libfat.a
link $t/fat.o 2> $t/log
grep -q "ignoring file '$t/fat.o': fat file missing arch 'x86_64', file has 'x86_64h'" $t/log
link $t/fat.o -Wl,-allow_sub_type_mismatches 2> $t/log
[ "$(grep -c "warning: linking x86_64h file '$t/fat.o' into x86_64 link" $t/log)" = 2 ]
nm $t/exe | grep -q ' T _foo$'
link $t/libfat.a -Wl,-allow_sub_type_mismatches,-u,_foo 2> $t/log
[ "$(grep -c "warning: linking x86_64h file '$t/libfat.a(b.o)' into x86_64 link" $t/log)" = 2 ]
nm $t/exe | grep -q ' T _foo$'
