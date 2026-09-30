#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without -platform_version, a link is for the deployment target of the
# first object file on the command line that records one: its platform,
# minimum OS and SDK, whatever the later objects say. Everything that
# depends on the deployment target follows from it (here, chained
# fixups from macOS 12).
sdk=$(xcrun --show-sdk-path)
for v in '11, 0' '13, 1' '14, 2'; do
  n=${v/, /}
  cat <<EOF | $CC -o $t/v$n.o -c -xassembler -
.build_version macos, $v sdk_version 15, 0
.globl _f$n
.p2align 2
_f$n: ret
.data
.p2align 3
.quad _f$n
EOF
done

$mold -arch $ARCH -dylib $t/v131.o $t/v110.o $t/v142.o -syslibroot $sdk -lSystem \
  -o $t/a.dylib 2> $t/log
grep -q "(.*v142.o) was built for newer 'macOS' version (14.2) than being linked (13.1)" $t/log
otool -l $t/a.dylib > $t/lc
grep -A4 'cmd LC_BUILD_VERSION' $t/lc > $t/bv
grep -q 'minos 13.1' $t/bv
grep -q 'sdk 15.0' $t/bv
grep -q LC_DYLD_CHAINED_FIXUPS $t/lc

$mold -arch $ARCH -dylib $t/v110.o $t/v131.o -syslibroot $sdk -lSystem -o $t/b.dylib 2> /dev/null
otool -l $t/b.dylib > $t/lc2
grep -A4 'cmd LC_BUILD_VERSION' $t/lc2 | grep -q 'minos 11.0'
not grep -q LC_DYLD_CHAINED_FIXUPS $t/lc2

# An object without a platform load command doesn't count: it is taken
# for macOS with a warning. Nor do archive members, even loaded ones,
# or universal files. With nothing to go by, a final link fails; a -r
# output records no platform.
cat <<EOF | $CC -target $ARCH-apple-macho -o $t/none.o -c -xassembler -
.globl _none
_none: ret
EOF
rm -f $t/lib.a
ar rcs $t/lib.a $t/v110.o
$mold -arch $ARCH -dylib $t/none.o $t/lib.a $t/v131.o -u _f110 -syslibroot $sdk -lSystem \
  -o $t/c.dylib 2> $t/log3
grep -q "no platform load command found in '.*none.o', assuming: macOS" $t/log3
otool -l $t/c.dylib | grep -A4 'cmd LC_BUILD_VERSION' | grep -q 'minos 13.1'

not $mold -arch $ARCH -dylib $t/lib.a $t/none.o -u _f110 -syslibroot $sdk -lSystem \
  -o $t/d.dylib 2> $t/log4
grep -q 'Missing -platform_version option' $t/log4

lipo -create $t/v131.o -output $t/fat.o
not $mold -arch $ARCH -dylib $t/fat.o -syslibroot $sdk -lSystem -o $t/d.dylib 2> $t/log4
grep -q 'Missing -platform_version option' $t/log4

# (With no inputs at all, that is what ld-prime reports first.)
not $mold -arch $ARCH -dylib -o $t/d.dylib 2> $t/log4
grep -q 'no object files specified' $t/log4

$mold -arch $ARCH -r $t/none.o -o $t/r.o
otool -l $t/r.o > $t/lc5
not grep -q 'cmd LC_BUILD_VERSION' $t/lc5

# Firmware objects make a firmware image, which needs no libSystem.
cat <<EOF | $CC -target $ARCH-apple-firmware1.0 -o $t/fw.o -c -xassembler - \
  -Wno-incompatible-sysroot
.globl _start
_start: ret
EOF
$mold -arch $ARCH -e _start $t/fw.o -o $t/fw
otool -l $t/fw > $t/lc6
not grep -q 'cmd LC_BUILD_VERSION' $t/lc6
not grep -q 'cmd LC_LOAD_DYLIB' $t/lc6

# The legacy command names the deployment target too; an x86-64 image
# for a macOS older than 10.14 records it the same way.
if [ $ARCH = x86_64 ]; then
  echo '.macosx_version_min 10, 13 sdk_version 15, 0' | \
    $CC -o $t/old.o -c -xassembler -
  $mold -arch $ARCH -dylib $t/old.o -syslibroot $sdk -lSystem -o $t/e.dylib
  otool -l $t/e.dylib > $t/lc7
  grep -A3 'cmd LC_VERSION_MIN_MACOSX' $t/lc7 | grep -q 'version 10.13'
  not grep -q 'cmd LC_BUILD_VERSION' $t/lc7
fi

# A bitcode file counts only if no Mach-O object does: then its target
# triple names the OS version, and no SDK.
lto_library=$(dirname "$(xcrun -f clang)")/../lib/libLTO.dylib
echo 'int lto(void) { return 1; }' | $CC -flto -mmacosx-version-min=12.3 -o $t/lto.o -c -xc -
$mold -arch $ARCH -dylib -lto_library $lto_library $t/lto.o -syslibroot $sdk -lSystem \
  -o $t/f.dylib
otool -l $t/f.dylib | grep -A4 'cmd LC_BUILD_VERSION' > $t/bv8
grep -q 'minos 12.3' $t/bv8
grep -q 'sdk n/a' $t/bv8

$mold -arch $ARCH -dylib -lto_library $lto_library $t/lto.o $t/v131.o -syslibroot $sdk \
  -lSystem -o $t/g.dylib 2> /dev/null
otool -l $t/g.dylib | grep -A4 'cmd LC_BUILD_VERSION' | grep -q 'minos 13.1'
