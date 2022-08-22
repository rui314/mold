#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int foo();
int main() { foo(); }
EOF

cat > $t/b.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-bar, arm64-bar ]
uuids:
  - target:          x86_64-bar
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-bar
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/lib/bar'
current-version: 0
compatibility-version: 0
exports:
  - targets:         [ x86_64-bar ]
    symbols:         [ _foo ]
  - targets:         [ arm64-bar ]
    symbols:         [ _foo ]
...
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.tbd

echo OK
