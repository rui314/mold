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
  jump=jmp
  ;;
aarch64)
  jump=b
  ;;
*)
  echo skipped
  exit 0
  ;;
esac

cat <<EOF | cc -o "$t"/a.o -c -x assembler -
foo: $jump 0
EOF

cat <<EOF | cc -o "$t"/b.o -c -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o

echo OK
