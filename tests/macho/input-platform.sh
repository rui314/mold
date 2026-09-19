#!/bin/bash
source "$(dirname "$0")"/common.inc

sdk=$(xcrun --show-sdk-path)

link() {
  "$mold" -arch $ARCH -dylib -platform_version macos 13.0 13.0 \
    -syslibroot "$sdk" -lSystem "$@"
}

echo 'int native(void) { return 0; }' | \
  $CC -target $ARCH-apple-macos13.0 -isysroot / -c -xc - -o $t/native.o
link $t/native.o -o $t/native.dylib 2> $t/native.log
test ! -s $t/native.log

# Modern simulator objects use LC_BUILD_VERSION. Older iOS objects use
# LC_VERSION_MIN_IPHONEOS (which denotes the simulator on x86_64).
# No SDK headers or libraries are needed to compile these fixtures.
for target in $ARCH-apple-ios14.0-simulator $ARCH-apple-ios11.0; do
  echo 'int other(void) { return 42; }' | \
    $CC -target $target -isysroot / -c -xc - -o $t/other.o

  # An incompatible archive member must not affect a link that does not
  # extract it, even though mold parses every member eagerly.
  rm -f $t/libother.a
  ar rcs $t/libother.a $t/other.o
  link $t/native.o $t/libother.a -o $t/unused.dylib 2> $t/unused.log
  test ! -s $t/unused.log

  # The same CPU architecture does not make iOS code valid for macOS.
  not link $t/native.o $t/other.o -o $t/direct.dylib 2> $t/direct.log
  grep -q 'macOS.*iOS' $t/direct.log
  grep -Fq 'other.o' $t/direct.log

  not link $t/native.o $t/libother.a -u _other \
    -o $t/archive.dylib 2> $t/archive.log
  grep -q 'macOS.*iOS' $t/archive.log
  grep -Fq 'libother.a' $t/archive.log
done
