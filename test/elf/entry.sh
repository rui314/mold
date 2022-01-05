#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

case "$(uname -m)" in
i386 | i686 | x86_64)
  base=0x201000
  ;;
aarch64)
  base=0x210000
  ;;
*)
  echo skipped
  exit 0
  ;;
esac

cat <<EOF | cc -o "$t"/a.o -c -x assembler -
.globl foo, bar
foo:
  .quad 0
bar:
  .quad 0
EOF

"$mold" -e foo -static -o "$t"/exe "$t"/a.o
readelf -e "$t"/exe > "$t"/log
grep -q "Entry point address:.*$base" "$t"/log

"$mold" -e bar -static -o "$t"/exe "$t"/a.o
readelf -e "$t"/exe > "$t"/log
grep -q "$(printf 'Entry point address:.*0x%x' $((base + 8)))" "$t"/log

"$mold" -static -o "$t"/exe "$t"/a.o
readelf -e "$t"/exe > "$t"/log
grep -q "Entry point address:.*$base" "$t"/log

echo OK
