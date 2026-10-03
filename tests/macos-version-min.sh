#!/bin/bash
source "$(dirname "$0")"/common.inc

# LC_BUILD_VERSION came with macOS 10.14. For an x86_64 target older
# than that, ld-prime writes the legacy LC_VERSION_MIN_MACOSX {version,
# sdk} in its place, in a final image and in -r output alike; arm64 gets
# LC_BUILD_VERSION at any version.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# The legacy flag carries no separate SDK; ld64 records it as the
# version.
$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-macos_version_min,10.9
otool -l $t/exe1 > $t/log
if [ $ARCH = x86_64 ]; then
  $t/exe1
  grep -A3 'cmd LC_VERSION_MIN_MACOSX' $t/log > $t/vmin
  grep -q 'version 10.9' $t/vmin
  grep -q 'sdk 10.9' $t/vmin
  not grep -q LC_BUILD_VERSION $t/log
else
  grep -q 'platform 1' $t/log
  grep -q 'minos 10.9' $t/log
  grep -q 'sdk 10.9' $t/log
fi

$CC --ld-path=$mold -o $t/exe2 $t/a.o -mmacosx-version-min=10.14
otool -l $t/exe2 > $t/log2
grep -q 'cmd LC_BUILD_VERSION' $t/log2
not grep -q LC_VERSION_MIN $t/log2

# Any version from 10.14 on gets LC_BUILD_VERSION. (ld-prime also
# takes the legacy command for 10.14.4 up to 10.15 and for 10.15.4 up to
# 10.16, from a slip in its version-to-year mapping.)
for v in 10.14.3 10.14.6 10.15 10.15.99 10.16; do
  $mold -arch $ARCH -r $t/a.o -platform_version macos $v 10.14 -o $t/r.o 2> /dev/null
  otool -l $t/r.o | awk '$1 == "cmd" && /VERSION/ { print $2 }' > $t/cmd
  grep -q LC_BUILD_VERSION $t/cmd
done

$CC -o $t/b.o -c -xc /dev/null -mmacosx-version-min=10.13
$mold -arch $ARCH -r $t/b.o -o $t/r.o
otool -l $t/r.o > $t/log3
if [ $ARCH = x86_64 ]; then
  grep -A3 'cmd LC_VERSION_MIN_MACOSX' $t/log3 | grep -q 'version 10.13'
  not grep -q LC_BUILD_VERSION $t/log3
else
  grep -q 'cmd LC_BUILD_VERSION' $t/log3
fi

# -macosx_version_min is the option's old spelling.
sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" -lSystem $t/a.o "$@"; }
link -macosx_version_min 14.1 -o $t/exe3
otool -l $t/exe3 | grep -A4 LC_BUILD_VERSION | grep -q 'minos 14.1'
not link -macosx_version_min 1x -o $t/exe3 2> $t/log5
grep -q -- "malformed 32-bit xxxx.yy.zz version number: '1x'" $t/log5
