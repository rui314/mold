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

# -fPIC is incompatible with -mcmodel=large on some targets
[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

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

$CXX -B. -o $t/exe -fPIC -pie $t/a.cc -mcmodel=large
$t/exe

$CXX -B. -o $t/exe -fPIC -pie $t/a.cc -static -mcmodel=large
$t/exe

echo OK
