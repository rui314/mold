#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF > "$t"/a.ver
{
  global:
  extern "C++" {
    foo::bar;
  };

  local: *;
};
EOF

cat <<EOF | c++ -fPIC -c -o "$t"/b.o -x c++ -
int bar = 5;
namespace foo {
int bar = 7;
}

int main() {
  return 0;
}
EOF

clang -fuse-ld="$mold" -shared -o "$t"/c.so -Wl,-version-script,"$t"/a.ver "$t"/b.o

readelf --dyn-syms "$t"/c.so > "$t"/log
fgrep -q _ZN3foo3barE "$t"/log
! fgrep -q ' bar' "$t"/log || false

echo OK
