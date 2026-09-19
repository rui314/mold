#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 0; }' | $CC -mmacosx-version-min=13.0 \
  --ld-path=$mold -dynamiclib -xc - -o $t/libfoo.dylib
cp $t/libfoo.dylib $t/original.dylib
echo 'int foo(); int main() { return foo(); }' | \
  $CC -mmacosx-version-min=13.0 -c -xc - -o $t/main.o

for cmd in -set-build-version -set-version-min; do
  for version in 12 13 14; do
    xcrun vtool $cmd macos $version.0 15.0 -replace \
      -output $t/libfoo.dylib $t/original.dylib
    $CC --ld-path=$mold -mmacosx-version-min=13.0 $t/main.o \
      $t/libfoo.dylib -o $t/exe 2> $t/log
    if [ $version = 14 ]; then
      grep -Eq 'warning:.*13.0.*libfoo.dylib.*14.0' $t/log
    else
      test ! -s $t/log
    fi
  done
done
