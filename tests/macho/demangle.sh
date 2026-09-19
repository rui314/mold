#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo(int); int main() { return foo(42); }' |
  $CXX -mmacosx-version-min=13.0 -c -xc++ - -o $t/a.o
echo 'int foo(int x) { return x; }' |
  $CXX -mmacosx-version-min=13.0 -c -xc++ - -o $t/b.o
cp $t/b.o $t/c.o
sdk=$(xcrun --show-sdk-path)

for opt in '' -demangle; do
  if [ "$opt" = -demangle ]; then
    name='foo(int)'
  else
    name=__Z3fooi
  fi
  not $mold -arch $ARCH -platform_version macos 13 13 -syslibroot "$sdk" \
    -lSystem $t/a.o $opt -o $t/exe 2> $t/log
  grep -Fq "$name" $t/log

  not $mold -arch $ARCH -platform_version macos 13 13 -syslibroot "$sdk" \
    -lSystem $t/a.o $t/b.o $t/c.o $opt -o $t/exe 2> $t/log
  grep -Fq "$name" $t/log
done
