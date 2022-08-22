#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

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
    symbols:         [ '$ld$hide$os25.0$_foo', _foo ]
...
EOF

cat <<EOF | cc -o $t/a.o -c -xc -
void foo();
int main() { foo(); }
EOF

cc --ld-path=./ld64 -o $t/exe $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,20.0,20.0 >& /dev/null

! cc --ld-path=./ld64 -o $t/exe $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,25.0,21.0 >& /dev/null || false

echo OK
