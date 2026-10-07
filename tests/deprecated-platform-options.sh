#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime deprecates, naming the platform, -flat_namespace everywhere
# but macOS; -undefined dynamic_lookup (or suppress) in an image dyld
# loads but on macOS and firmware; and -bind_at_load in an image with
# chained fixups, which dyld binds at load anyway, on macOS too. It
# counts a -preload image as firmware's, and a -r output for no
# platform.
cat <<EOF > $t/a.c
int main(void) { return 0; }
EOF

mac=$(xcrun --sdk macosx --show-sdk-path)
$CC -mmacos-version-min=11.0 -o $t/mac.o -c $t/a.c
link() { $mold -arch $ARCH -o $t/out "$@" 2> $t/log; }
mac() { link -syslibroot $mac -platform_version macos "$@"; }

mac 15.0 15.0 -dylib $t/mac.o -lSystem -flat_namespace -undefined dynamic_lookup
not grep -q deprecated $t/log
mac 15.0 15.0 -dylib $t/mac.o -lSystem -bind_at_load
grep -q -- '-bind_at_load is deprecated on macOS' $t/log
mac 11.0 15.0 -dylib $t/mac.o -lSystem -bind_at_load
not grep -q deprecated $t/log
mac 11.0 15.0 -dylib $t/mac.o -lSystem -bind_at_load -fixup_chains
grep -q -- '-bind_at_load is deprecated on macOS' $t/log
mac 15.0 15.0 -dylib $t/mac.o -lSystem -bind_at_load -no_fixup_chains
not grep -q deprecated $t/log
mac 15.0 15.0 -dylib $t/mac.o -lSystem -bind_at_load -undefined dynamic_lookup
not grep -q deprecated $t/log
mac 15.0 15.0 -r $t/mac.o -bind_at_load
not grep -q deprecated $t/log
mac 15.0 15.0 -r $t/mac.o -bind_at_load -fixup_chains
grep -q -- '-bind_at_load is deprecated on macOS' $t/log
mac 15.0 15.0 -static -e _main $t/mac.o -bind_at_load
not grep -q deprecated $t/log

mac 15.0 15.0 -preload -e _main $t/mac.o -flat_namespace -bind_at_load -fixup_chains \
  -undefined dynamic_lookup
grep -q -- '-flat_namespace is deprecated on firmware' $t/log
grep -q -- '-bind_at_load is deprecated on firmware' $t/log
not grep -q dynamic_lookup $t/log

link -platform_version firmware 1.0 1.0 -dylib $t/mac.o -flat_namespace -bind_at_load \
  -fixup_chains -undefined dynamic_lookup
grep -q -- '-flat_namespace is deprecated on firmware' $t/log
grep -q -- '-bind_at_load is deprecated on firmware' $t/log
not grep -q dynamic_lookup $t/log

cat <<EOF | $CC -target $ARCH-apple-macho -o $t/none.o -c -xassembler -
.globl _none
_none: ret
EOF
link -r $t/none.o -flat_namespace
grep -q -- '-flat_namespace is deprecated on firmware' $t/log

if sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null); then
  cc -target $ARCH-apple-ios17.0-simulator -isysroot $sdk -o $t/sim.o -c $t/a.c
  sim() { link -syslibroot $sdk -platform_version ios-simulator 17.0 27.0 "$@"; }

  sim -dylib $t/sim.o -lSystem -flat_namespace -undefined dynamic_lookup
  grep -q -- '-flat_namespace is deprecated on iOS-simulator' $t/log
  grep -q -- '-undefined dynamic_lookup is deprecated on iOS-simulator' $t/log
  sim $t/sim.o -lSystem -undefined suppress
  grep -q -- '-undefined suppress is deprecated' $t/log
  grep -q -- '-undefined dynamic_lookup is deprecated on iOS-simulator' $t/log
  sim -bundle $t/sim.o -lSystem -bind_at_load -fixup_chains
  grep -q -- '-bind_at_load is deprecated on iOS-simulator' $t/log
  sim -r $t/sim.o -flat_namespace -undefined dynamic_lookup
  grep -q -- '-flat_namespace is deprecated on iOS-simulator' $t/log
  not grep -q dynamic_lookup $t/log
  sim -static -e _main $t/sim.o -undefined dynamic_lookup
  not grep -q dynamic_lookup $t/log
fi

if [ $ARCH = arm64 ]; then
  for p in ios:iOS tvos:tvOS xros:visionOS; do
    os=${p%:*}
    cc -target arm64-apple-${os}2.0 -o $t/$os.o -c $t/a.c
    link -platform_version $os 17.0 27.0 -r $t/$os.o -flat_namespace
    grep -q -- "-flat_namespace is deprecated on ${p#*:}\$" $t/log
  done
fi
