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
    symbols:         [ '$ld$add$os14.0$_foo' ]
...
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo();
void bar() { foo(); }
EOF

! $CC --ld-path=$mold -shared -o $t/b.dylib $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,9.0,9.0 >& /dev/null || false

$CC --ld-path=$mold -shared -o $t/b.dylib $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,14.0,13.0 >& /dev/null
