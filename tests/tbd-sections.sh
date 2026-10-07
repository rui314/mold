#!/bin/bash
source "$(dirname "$0")"/common.inc

# A .tbd lists what the library exports in its "exports" sections; the
# symbols its "undefineds" sections list are what it imports, which no
# client can link against. A sequence may be written as a block, one
# "- item" a line, as well as in flow style. Version 1 spells the
# clients a section allows allowed-clients.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF

cat > $t/libundef3.tbd <<EOF
--- !tapi-tbd-v3
archs:           [ $ARCH ]
platform:        macosx
install-name:    '/usr/lib/libundef3.dylib'
undefineds:
  - archs:           [ $ARCH ]
    symbols:         [ _foo ]
...
EOF
not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libundef3.tbd 2> $t/log
grep -q _foo $t/log

cat > $t/libundef4.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '/usr/lib/libundef4.dylib'
undefineds:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _foo ]
...
EOF
not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libundef4.tbd 2> $t/log
grep -q _foo $t/log

cat > $t/libblock.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '/usr/lib/libblock.dylib'
exports:
  - targets:
      - $ARCH-macos
    symbols:
      - _bar
      - '_foo'
...
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libblock.tbd
dyld_info -fixups $t/exe | grep -q 'libblock/_foo'

cat > $t/libv1.tbd <<EOF
---
archs:           [ $ARCH ]
platform:        macosx
install-name:    '/usr/lib/libv1.dylib'
exports:
  - archs:           [ $ARCH ]
    allowed-clients: [ Friend ]
    symbols:         [ _foo ]
...
EOF
not $CC --ld-path=$mold -o $t/exe $t/a.o $t/libv1.tbd 2> $t/log
grep -q "cannot link directly with 'libv1.dylib' because product being built is not an allowed client of it" $t/log
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libv1.tbd -Wl,-client_name,Friend
