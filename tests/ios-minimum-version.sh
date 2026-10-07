#!/bin/bash
source "$(dirname "$0")"/common.inc

# iOS 4.3 is the oldest iOS, and the oldest iOS simulator, a link may
# target, however the options or the first object name the version;
# a -r output can't be for an older one either, but a -preload image,
# which no iOS loads, can. tvOS and visionOS take any version.
cat <<EOF > $t/a.c
int main(void) { return 0; }
EOF

if [ $ARCH = arm64 ] && sdk=$(xcrun --sdk iphoneos --show-sdk-path 2> /dev/null); then
  cc -target arm64-apple-ios17.0 -isysroot $sdk -o $t/ios.o -c $t/a.c
  cc -target arm64-apple-ios4.2 -isysroot $sdk -o $t/ios42.o -c $t/a.c
  link() { $mold -syslibroot $sdk -o $t/exe "$@"; }

  for version in 4.2.255 4.2 4 0.0; do
    not link -arch arm64 -platform_version ios $version 27.0 $t/ios.o -lSystem 2> $t/log
    grep -q "building for iOS with $version.* minimum deployment target is no longer supported" \
      $t/log
  done
  not link -arch arm64 -platform_version ios 4.2 27.0 -r $t/ios.o 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log
  not link -arch arm64 -platform_version ios 4.2 27.0 -dylib $t/ios.o -lSystem 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log
  not link -arch arm64 -platform_version ios 4.2 27.0 -static -e _main $t/ios.o 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log
  not link -arch arm64 -ios_version_min 4.2 $t/ios.o -lSystem 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log
  not link -target arm64-apple-ios4.2 $t/ios.o -lSystem 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log
  not link -arch arm64 $t/ios42.o -lSystem 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log
  not link -arch arm64 -r $t/ios42.o 2> $t/log
  grep -q 'building for iOS with 4.2 minimum' $t/log

  link -arch arm64 -platform_version ios 4.3 27.0 $t/ios.o -lSystem
  link -arch arm64 -platform_version ios 4.2 27.0 -preload -e _main $t/ios.o
fi

if sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null); then
  cc -target $ARCH-apple-ios17.0-simulator -isysroot $sdk -o $t/sim.o -c $t/a.c
  not $mold -arch $ARCH -platform_version ios-simulator 4.2 27.0 -syslibroot $sdk -o $t/exe \
    $t/sim.o -lSystem 2> $t/log
  grep -q 'building for iOS with 4.2 minimum deployment target is no longer supported' $t/log
  # (An x86_64 executable that old starts at "start", which a.c lacks.)
  $mold -arch $ARCH -platform_version ios-simulator 4.3 27.0 -syslibroot $sdk -o $t/lib.dylib \
    -dylib $t/sim.o -lSystem
fi

for p in tvos:appletvos:17.0 tvos-simulator:appletvsimulator:17.0 xros:xros:2.0 \
  xros-simulator:xrsimulator:2.0; do
  IFS=: read name sdkname ver <<< "$p"
  [ $ARCH = arm64 ] || [[ $name = *-simulator ]] || continue
  sdk=$(xcrun --sdk $sdkname --show-sdk-path 2> /dev/null) || continue
  os=${name%-simulator}
  cc -target $ARCH-apple-$os$ver${name#$os} -isysroot $sdk -o $t/$name.o -c $t/a.c
  $mold -arch $ARCH -platform_version $name 0.0 27.0 -syslibroot $sdk -o $t/lib.dylib \
    -dylib $t/$name.o -lSystem
done
