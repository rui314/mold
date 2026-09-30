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

$CC -o $t/b.o -c -xc /dev/null -mmacosx-version-min=10.13
$mold -arch $ARCH -r $t/b.o -o $t/r.o
otool -l $t/r.o > $t/log3
if [ $ARCH = x86_64 ]; then
  grep -A3 'cmd LC_VERSION_MIN_MACOSX' $t/log3 | grep -q 'version 10.13'
  not grep -q LC_BUILD_VERSION $t/log3
else
  grep -q 'cmd LC_BUILD_VERSION' $t/log3
fi
