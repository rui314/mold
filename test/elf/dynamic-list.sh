#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
void foo() {}
void bar() {}
int main() {}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o

readelf --dyn-syms "$t"/exe > "$t"/log
! grep -q ' foo$' "$t"/log || false
! grep -q ' bar$' "$t"/log || false

cat <<EOF > "$t"/dyn
{ foo; bar; };
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-dynamic-list="$t"/dyn

readelf --dyn-syms "$t"/exe > "$t"/log
grep -q ' foo$' "$t"/log
grep -q ' bar$' "$t"/log

echo OK
