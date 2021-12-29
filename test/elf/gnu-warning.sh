#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | gcc -c -o "$t"/a.o -xc -
void foo() {}

__attribute__((section(".gnu.warning.foo")))
static const char foo_warning[] = "warning message";
EOF

cat <<EOF | cc -c -o "$t"/b.o -xc -
void foo();

int main() { foo(); }
EOF

# Make sure that we do not copy .gnu.warning.* sections.
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o
! readelf --sections "$t"/exe | fgrep -q .gnu.warning || false

echo OK
