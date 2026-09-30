#!/bin/bash
source "$(dirname "$0")"/common.inc

# A .tbd file lists the targets (architecture-platform pairs) its
# library is for. The link reads the one for its own platform. A
# firmware link takes a library of any other platform, with a warning
# naming the stub's platforms - "macOS macCatalyst
# zippered(macOS/Catalyst)" for the SDK's zippered libraries - and a
# macOS link refuses one without a macOS target.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  ret
.data
.p2align 3
_p: .quad _foo
EOF

tbd() {
  cat > $t/$1.tbd <<EOF
--- !tapi-tbd
tbd-version: 4
targets: [ $2 ]
install-name: '/usr/lib/lib$1.dylib'
exports:
  - targets: [ $2 ]
    symbols: [ _foo ]
...
EOF
}
tbd mac "$ARCH-macos"
tbd zip "$ARCH-macos, $ARCH-maccatalyst"
tbd ios "$ARCH-ios, $ARCH-ios-simulator"
tbd fw "$ARCH-macos, $ARCH-firmware"

fw='-platform_version firmware 1.0 1.0'
$mold -arch $ARCH $fw -e _start $t/a.o $t/mac.tbd -o $t/exe1 2> $t/log1
grep -q "building for 'firmware', but linking in dylib (/.*/$t/mac.tbd) built for 'macOS'$" $t/log1

$mold -arch $ARCH $fw -e _start $t/a.o $t/zip.tbd -o $t/exe2 2> $t/log2
grep -q "built for 'macOS macCatalyst zippered(macOS/Catalyst)'$" $t/log2

$mold -arch $ARCH $fw -e _start $t/a.o $t/ios.tbd -o $t/exe3 2> $t/log3
grep -q "built for 'iOS iOS-simulator'$" $t/log3

$mold -arch $ARCH $fw -e _start $t/a.o $t/fw.tbd -o $t/exe4 2> $t/log4
not grep -q warning $t/log4
otool -L $t/exe4 | grep -q /usr/lib/libfw.dylib

sdk=$(xcrun --show-sdk-path)
mac="-platform_version macos 26.0 26.0 -syslibroot $sdk -lSystem"
$mold -arch $ARCH $mac -e _start $t/a.o $t/zip.tbd -o $t/exe5 2> $t/log5
not grep -q 'building for' $t/log5
not $mold -arch $ARCH $mac -e _start $t/a.o $t/ios.tbd -o $t/exe6 2> $t/log6
grep -q "building for 'macOS', but linking in dylib (/.*/$t/ios.tbd) built for 'iOS iOS-simulator'$" $t/log6
