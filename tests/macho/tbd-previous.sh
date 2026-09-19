#!/bin/bash
source "$(dirname "$0")"/common.inc

cat > $t/libfoo.tbd <<'EOF'
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/foo'
current-version: 0
compatibility-version: 0
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ '$ld$previous$/bar$$1$10.0$15.0$$', _foo ]
...
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo();
void bar() { foo(); }
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,9.0,9.0 2> /dev/null

otool -L $t/b.dylib | grep -q /foo

$CC --ld-path=$mold -shared -o $t/b.dylib $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,14.0,14.0 2> /dev/null

otool -L $t/b.dylib | grep -q /bar
