#!/bin/bash
source "$(dirname "$0")"/common.inc

# The test picks the platforms it links for itself.
on_simulator && skip

# An object with no platform load command (an old one, or one assembled
# for no OS) is taken for the link's platform, whichever that is, with
# a warning that names it - but in firmware, which takes code built for
# any platform, and so in a -preload image or a -r output for no
# platform, which ld-prime counts as firmware's.
cat <<EOF | $CC -target $ARCH-apple-macho -o $t/none.o -c -xassembler -
.globl _main
_main: ret
EOF

link() { $mold -arch $ARCH -o $t/out $t/none.o "$@" 2> $t/log; }

for p in macos:macOS ios-simulator:iOS-simulator tvos-simulator:tvOS-simulator \
  xros-simulator:visionOS-simulator ios:iOS tvos:tvOS xros:visionOS; do
  [ $ARCH = arm64 ] || [[ $p = *-simulator:* ]] || [[ $p = macos:* ]] || continue
  link -r -platform_version ${p%:*} 17.0 27.0
  grep -q "no platform load command found in '.*none.o', assuming: ${p#*:}\$" $t/log
  link -static -e _main -platform_version ${p%:*} 17.0 27.0
  grep -q "no platform load command found in '.*none.o', assuming: ${p#*:}\$" $t/log
  link -preload -e _main -platform_version ${p%:*} 17.0 27.0
  not grep -q 'no platform load command' $t/log
done

link -r
not grep -q 'no platform load command' $t/log
link -static -e _main -platform_version firmware 1.0 1.0
not grep -q 'no platform load command' $t/log
link -dylib -platform_version firmware 1.0 1.0
not grep -q 'no platform load command' $t/log
