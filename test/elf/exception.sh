#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF > "$t"/a.cc
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/a.cc -static
"$t"/exe

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/a.cc
"$t"/exe

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/a.cc -Wl,--gc-sections
"$t"/exe

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/a.cc -static -Wl,--gc-sections
"$t"/exe

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/a.cc -static -mcmodel=large
"$t"/exe

echo OK
