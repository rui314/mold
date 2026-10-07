#!/bin/bash
source "$(dirname "$0")"/common.inc

# A simulator's image has 16 bytes more free space after its load
# commands than a macOS one with the same dylibs, on top of -headerpad
# or -headerpad_max_install_names too, as ld-prime leaves it.
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null) || skip

# The free space between the load commands and the first section.
slack() {
  local cmds=$(otool -h $1 | awk 'NR == 4 { print $7 }')
  local first=$(otool -l $1 | awk '$1 == "offset" && $2 > 0 { print $2 }' | sort -n | head -1)
  echo $((first - 32 - cmds))
}

cat <<EOF > $t/a.c
int main(void) { return 0; }
EOF
$CC -mmacos-version-min=14.0 -o $t/mac.o -c $t/a.c
cc -target $ARCH-apple-ios17.0-simulator -isysroot $sdk -o $t/sim.o -c $t/a.c

for opts in -execute -dylib -bundle '-headerpad 0x100' -headerpad_max_install_names \
  '-static -e _main'; do
  lib=-lSystem
  [[ $opts = -static* ]] && lib=
  $mold -arch $ARCH -platform_version macos 14.0 15.0 -syslibroot $(xcrun --sdk macosx \
    --show-sdk-path) -o $t/mac $opts $t/mac.o $lib
  $mold -arch $ARCH -platform_version ios-simulator 17.0 27.0 -syslibroot $sdk -o $t/sim \
    $opts $t/sim.o $lib
  [ $(slack $t/sim) = $(($(slack $t/mac) + 16)) ]
done
