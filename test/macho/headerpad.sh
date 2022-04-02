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

[ "`uname -p`" = arm ] && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o -Wl,-headerpad,0
otool -l $t/exe | grep -A5 'sectname __text' | grep -q 'addr 0x0000000100000570'

clang -fuse-ld="$mold" -o $t/exe $t/a.o -Wl,-headerpad,0x10000
otool -l $t/exe | grep -A5 'sectname __text' | grep -q 'addr 0x0000000100010570'

echo OK
