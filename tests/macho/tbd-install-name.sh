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
    symbols:         [ '$ld$install_name$os25.0$/bar', _foo ]
...
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC --ld-path=$mold -o $t/exe1 $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,20.0,20.0 >& /dev/null

otool -L $t/exe1 | grep -q /foo

$CC --ld-path=$mold -o $t/exe2 $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,25.0,21.0 >& /dev/null

otool -L $t/exe2 | grep -q /bar
