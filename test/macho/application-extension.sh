#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<'EOF' > $t/a.tbd
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/frameworks/SomeFramework.framework/SomeFramework'
current-version: 0000
compatibility-version: 150
flags: [ not_app_extension_safe ]
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo ]
...
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
int foo();
int main() { foo(); }
EOF

cc -o $t/exe1 $t/b.o $t/a.tbd >& $t/log1
! grep -q 'application extension' $t/log1 || false

cc -o $t/exe1 $t/b.o $t/a.tbd -Wl,-application_extension >& $t/log2
grep -q 'application extension' $t/log2

echo OK
