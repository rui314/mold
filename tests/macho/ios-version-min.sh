#!/bin/bash
source "$(dirname "$0")"/common.inc

# -ios_version_min names the iOS device as its deployment target (no
# such option names a simulator), with the SDK at the same version, as
# -macos_version_min does macOS. Its old name, -iphoneos_version_min,
# still works, with a warning. Mac Catalyst's options, under each of
# their names, are refused: mold doesn't link for it.
[ $ARCH = arm64 ] || skip
sdk=$(xcrun --sdk iphoneos --show-sdk-path 2> /dev/null) || skip

echo 'int main() { return 0; }' | cc -target arm64-apple-ios16.0 -isysroot $sdk -o $t/a.o \
  -c -xc -
link() { $mold -arch $ARCH -syslibroot $sdk -lSystem $t/a.o -o $t/exe "$@"; }
bv() {
  otool -l $1 | grep -A4 'cmd LC_BUILD_VERSION' |
    awk '$1 == "platform" || $1 == "minos" || $1 == "sdk" { printf "%s %s ", $1, $2 }'
}

link -ios_version_min 16.4 2> $t/log1
[ "$(bv $t/exe)" = 'platform 2 minos 16.4 sdk 16.4 ' ]
not grep -q renamed $t/log1

link -iphoneos_version_min 16.4 2> $t/log2
[ "$(bv $t/exe)" = 'platform 2 minos 16.4 sdk 16.4 ' ]
grep -q -- '-iphoneos_version_min has been renamed to -ios_version_min' $t/log2

not link -ios_version_min 2> $t/log3

for opt in -maccatalyst_version_min -iosmac_version_min -uikitformac_version_min; do
  not link $opt 16.0 2> $t/log4
  if $mold -v 2>&1 | grep -q mold-macho; then
    grep -q -- "$opt: unsupported platform" $t/log4
  fi
done
