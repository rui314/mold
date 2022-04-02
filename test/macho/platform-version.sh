#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t=out/test/macho/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o -Wl,-platform_version,macos,13.5,12.0

otool -l $t/exe > $t/log
fgrep -q 'minos 13.5' $t/log
fgrep -q 'sdk 12.0' $t/log

echo OK
