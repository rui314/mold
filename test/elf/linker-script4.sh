#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

echo 'VERSION { ver_x { global: *; }; };' > "$t"/a.script

cat <<EOF > "$t"/b.s
.globl foo, bar, baz
foo:
  nop
bar:
  nop
baz:
  nop
EOF

clang -fuse-ld="$mold" -shared -o "$t"/c.so "$t"/a.script "$t"/b.s
readelf --version-info "$t"/c.so > "$t"/log

fgrep -q 'Rev: 1  Flags: none  Index: 2  Cnt: 1  Name: ver_x' "$t"/log

echo OK
