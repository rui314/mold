#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
void fn1();
int main() {
  fn1();
}
EOF

cat <<EOF | cc -o "$t"/b.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int fn1() { return 42; }
EOF

cat <<EOF | cc -o "$t"/c.so -shared -fPIC -Wl,-soname,libbar.so -xc -
int fn2() { return 42; }
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.so "$t"/c.so

readelf --dynamic "$t"/exe > "$t"/readelf
fgrep -q 'Shared library: [libfoo.so]' "$t"/readelf
fgrep -q 'Shared library: [libbar.so]' "$t"/readelf

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,--as-needed "$t"/b.so "$t"/c.so

readelf --dynamic "$t"/exe > "$t"/readelf
fgrep -q 'Shared library: [libfoo.so]' "$t"/readelf
! fgrep -q 'Shared library: [libbar.so]' "$t"/readelf || false

echo OK
