#!/bin/bash
source "$(dirname "$0")"/common.inc

# A macOS dylib marked MH_SIM_SUPPORT (-simulator_support) may be loaded
# by a simulator's processes, which run on the Mac: it counts as built
# for macOS and every simulator, so a simulator's image links against
# it, whatever its macOS version, while a device's still may not.
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null) || skip
mac=$(xcrun --sdk macosx --show-sdk-path)

cat <<EOF | $CC -mmacos-version-min=14.0 -o $t/lib.o -c -xc -
int foo(void) { return 3; }
EOF
cat <<EOF > $t/main.c
int foo(void);
int main(void) { return foo(); }
EOF

for flag in -simulator_support ''; do
  $mold -arch $ARCH -platform_version macos 16.0 26.0 -syslibroot $mac -dylib \
    -install_name /usr/local/lib/libfoo.dylib -o $t/lib$flag.dylib $t/lib.o -lSystem $flag
done

cc -target $ARCH-apple-ios17.0-simulator -isysroot $sdk -o $t/sim.o -c $t/main.c
$mold -arch $ARCH -platform_version ios-simulator 17.0 27.0 -syslibroot $sdk -o $t/exe \
  $t/sim.o -lSystem $t/lib-simulator_support.dylib 2> $t/log
not grep -q libfoo $t/log
otool -L $t/exe | grep -q /usr/local/lib/libfoo.dylib

not $mold -arch $ARCH -platform_version ios-simulator 17.0 27.0 -syslibroot $sdk -o $t/exe \
  $t/sim.o -lSystem $t/lib.dylib 2> $t/log
grep -q "building for 'iOS-simulator', but linking in dylib (.*lib.dylib) built for 'macOS'" \
  $t/log

if sdk=$(xcrun --sdk appletvsimulator --show-sdk-path 2> /dev/null); then
  cc -target $ARCH-apple-tvos17.0-simulator -isysroot $sdk -o $t/tvsim.o -c $t/main.c
  $mold -arch $ARCH -platform_version tvos-simulator 17.0 27.0 -syslibroot $sdk -o $t/exe \
    $t/tvsim.o -lSystem $t/lib-simulator_support.dylib
fi

if [ $ARCH = arm64 ] && sdk=$(xcrun --sdk iphoneos --show-sdk-path 2> /dev/null); then
  cc -target arm64-apple-ios17.0 -isysroot $sdk -o $t/ios.o -c $t/main.c
  not $mold -arch arm64 -platform_version ios 17.0 27.0 -syslibroot $sdk -o $t/exe \
    $t/ios.o -lSystem $t/lib-simulator_support.dylib 2> $t/log
  grep -q "building for 'iOS', but linking in dylib (.*lib-simulator_support.dylib) built for 'macOS iOS-simulator .*tvOS-simulator.*'" \
    $t/log
fi
