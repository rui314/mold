#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'int main() { return 0; }' | $CC -c -xc - -o $t/a.o
sdk=$(xcrun --show-sdk-path)

# ld-prime vets -lto_library ahead of the rest of the command line: it
# runs itself again to load the library in place of its own, which
# must be named libLTO.dylib for that - every one given.
not $mold -arch $ARCH -platform_version macos 13.0 13.0 -syslibroot $sdk -lSystem \
  -lto_library $t/libfoo.dylib -lto_library $t/libLTO.dylib -bogus $t/a.o \
  -o $t/exe 2> $t/log1
grep -q -- "-lto_library library filename must be 'libLTO.dylib'" $t/log1
not grep -q unknown $t/log1

# The last one counts, and is ignored with a warning if it does not
# exist.
$mold -arch $ARCH -platform_version macos 13.0 13.0 -syslibroot $sdk -lSystem \
  -lto_library $t/x/libLTO.dylib -lto_library $t/y/libLTO.dylib $t/a.o \
  -o $t/exe 2> $t/log2
grep -q "warning: ignoring -lto_library '$t/y/libLTO.dylib', file does not exist" $t/log2
not grep -q "$t/x/" $t/log2
