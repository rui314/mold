#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | c++ -o "$t"/exe -Wl,-hash-style=gnu -xc++ -
#include <iostream>

int main() {
  std::cout << "foo\n";
}
EOF

"$t"/exe | grep -q foo

echo OK
