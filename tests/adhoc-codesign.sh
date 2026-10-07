#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -B. -o $t/exe1 $t/a.o -Wl,-adhoc_codesign
otool -l $t/exe1 | grep LC_CODE_SIGNATURE
$t/exe1 | grep -F 'Hello world'

$CC --ld-path=$mold -B. -o $t/exe2 $t/a.o -Wl,-no_adhoc_codesign
otool -l $t/exe2 > $t/log2
! grep -q LC_CODE_SIGNATURE $t/log2 || false
grep -q LC_UUID $t/log2
! grep -q 'uuid 00000000-0000-0000-0000-000000000000' $t/log2 || false

# The default follows ld-prime: arm64 output is ad-hoc signed, x86-64
# output is not (Intel Macs and Rosetta run unsigned code).
$CC --ld-path=$mold -B. -o $t/exe3 $t/a.o
otool -l $t/exe3 > $t/log3
if [ $ARCH = arm64 ]; then
  grep -q LC_CODE_SIGNATURE $t/log3
  codesign -v $t/exe3
else
  not grep -q LC_CODE_SIGNATURE $t/log3
fi
$t/exe3 | grep -F 'Hello world'

# So is a simulator's arm64 image, which Apple silicon runs too, but
# not a device's: packaging the app signs it with the developer's
# identity.
if sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null); then
  echo 'int main() { return 0; }' | \
    cc -target $ARCH-apple-ios17.0-simulator -isysroot $sdk -o $t/sim.o -c -xc -
  $mold -arch $ARCH -platform_version ios-simulator 17.0 27.0 -syslibroot $sdk -lSystem \
    $t/sim.o -o $t/sim
  otool -l $t/sim > $t/log4
  if [ $ARCH = arm64 ]; then
    grep -q LC_CODE_SIGNATURE $t/log4
    codesign -v $t/sim
  else
    not grep -q LC_CODE_SIGNATURE $t/log4
  fi
fi
if [ $ARCH = arm64 ] && sdk=$(xcrun --sdk iphoneos --show-sdk-path 2> /dev/null); then
  echo 'int main() { return 0; }' | \
    cc -target arm64-apple-ios17.0 -isysroot $sdk -o $t/dev.o -c -xc -
  $mold -arch $ARCH -platform_version ios 17.0 27.0 -syslibroot $sdk -lSystem $t/dev.o \
    -o $t/dev
  otool -l $t/dev > $t/log5
  not grep -q LC_CODE_SIGNATURE $t/log5
fi
