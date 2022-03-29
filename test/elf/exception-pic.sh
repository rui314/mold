#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF > $t/a.cc
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

$CXX -B. -o $t/exe -fPIC -pie $t/a.cc -static
$t/exe

$CXX -B. -o $t/exe -fPIC -pie $t/a.cc
$t/exe

$CXX -B. -o $t/exe -fPIC -pie $t/a.cc -Wl,--gc-sections
$t/exe

$CXX -B. -o $t/exe -fPIC -pie $t/a.cc -static -Wl,--gc-sections
$t/exe

echo OK
