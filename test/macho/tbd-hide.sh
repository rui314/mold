#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
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

cat <<EOF | clang -o $t/a.o -c -xc -
void foo();
int main() { foo(); }
EOF

clang --ld-path=./ld64 -o $t/exe $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,20.0,20.0 >& /dev/null

! clang --ld-path=./ld64 -o $t/exe $t/libfoo.tbd $t/a.o \
  -Wl,-platform_version,macos,25.0,21.0 >& /dev/null || false

echo OK
