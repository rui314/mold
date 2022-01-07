#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | $CC -o "$t"/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | $CC -o "$t"/b.o -c -xc -
void hello();
int main() { hello(); }
EOF

cat <<EOF > "$t"/filelist
a.o
b.o
EOF

clang -fuse-ld="$mold" -o "$t"/exe -Xlinker -filelist -Xlinker "$t"/filelist,"$t"
"$t"/exe | grep -q 'Hello world'

echo OK
