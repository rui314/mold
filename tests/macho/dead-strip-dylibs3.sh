#!/bin/bash
source "$(dirname "$0")"/common.inc

cat > $t/libfoo.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/lib/libfoo.dylib'
current-version: 0000
compatibility-version: 150
reexported-libraries:
  - targets:         [ x86_64-macos, arm64-macos ]
    libraries:       [ '/usr/lib/libbar.dylib' ]
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo ]
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/lib/libbar.dylib'
current-version: 0000
compatibility-version: 150
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _bar ]
...
EOF

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-lfoo
objdump --macho --dylibs-used $t/exe > $t/log
grep -q libfoo.dylib $t/log
! grep -q libbar.dylib $t/log || false
