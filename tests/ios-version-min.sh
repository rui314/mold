#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime takes iOS's and Mac Catalyst's minimum versions as it does
# macOS's, under their older names too. mold links for neither platform
# and refuses each option, with its version or without. (ld-prime
# refuses the macOS object file.)
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" -lSystem $t/a.o -o $t/exe "$@"; }

for opt in -ios_version_min -iphoneos_version_min -maccatalyst_version_min \
  -iosmac_version_min -uikitformac_version_min; do
  not link $opt 17.0 2> $t/log
  if $mold -v 2> /dev/null | grep -q mold-macho; then
    grep -q -- "$opt: unsupported platform" $t/log
  fi
  not link $opt 2> $t/log
done
