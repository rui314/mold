#!/bin/bash
source "$(dirname "$0")"/common.inc

sdk=$(xcrun --show-sdk-path)

link() {
  "$mold" -arch $ARCH -dylib -platform_version macos 13.0 13.0 \
    -syslibroot "$sdk" -lSystem "$@"
}

# Exercise both load command encodings explicitly. A newer SDK alone
# must not cause a deployment-target warning: every fixture records
# SDK 15.0, but only the object requiring macOS 14.0 should warn.
for format in build_version version_min; do
  if [ "$format" = build_version ]; then
    directive='.build_version macos,'
  else
    directive='.macosx_version_min'
  fi

  for version in 12 13 14; do
    cat <<EOF | $CC -c -xassembler - -o $t/macos$version.o
$directive $version, 0 sdk_version 15, 0
.text
.globl _from_macos_$version
.p2align 2
_from_macos_$version:
  ret
EOF
  done

  # Inputs targeting an older or equal macOS version link quietly.
  for version in 12 13; do
    link $t/macos$version.o -o $t/compatible.dylib 2> $t/compatible.log
    test ! -s $t/compatible.log
  done

  # Unused archive members must not produce deployment warnings.
  rm -f $t/libnewer.a
  ar rcs $t/libnewer.a $t/macos14.o
  link $t/macos13.o $t/libnewer.a -o $t/unused.dylib 2> $t/unused.log
  test ! -s $t/unused.log

  # A newer deployment target is a warning, so these links must still
  # succeed. Identify the input and both versions in the diagnostic.
  link $t/macos14.o -o $t/newer.dylib 2> $t/newer.log
  grep -Eq 'warning:.*newer.*macOS.*14\.0.*13\.0' $t/newer.log
  grep -Fq 'macos14.o' $t/newer.log

  link $t/macos13.o $t/libnewer.a -u _from_macos_14 \
    -o $t/archive.dylib 2> $t/archive.log
  grep -Eq 'warning:.*newer.*macOS.*14\.0.*13\.0' $t/archive.log
  grep -Fq 'libnewer.a' $t/archive.log
done
