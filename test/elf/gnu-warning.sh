#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | gcc -c -o "$t"/a.o -xc -
void foo() {}

__attribute__((section(".gnu.warning.foo")))
static const char foo_warning[] = "warning message";
EOF

cat <<EOF | $CC -c -o "$t"/b.o -xc -
void foo();

int main() { foo(); }
EOF

# Make sure that we do not copy .gnu.warning.* sections.
$CC -B. -o "$t"/exe "$t"/a.o "$t"/b.o
! readelf --sections "$t"/exe | fgrep -q .gnu.warning || false

echo OK
