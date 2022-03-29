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

$CXX -B. -o $t/exe $t/a.cc -static
$t/exe

$CXX -B. -o $t/exe $t/a.cc
$t/exe

$CXX -B. -o $t/exe $t/a.cc -Wl,--gc-sections
$t/exe

$CXX -B. -o $t/exe $t/a.cc -static -Wl,--gc-sections
$t/exe

if [ "$(uname -m)" = x86_64 ]; then
  $CXX -B. -o $t/exe $t/a.cc -mcmodel=large
  $t/exe

  $CXX -B. -o $t/exe $t/a.cc -static -mcmodel=large
  $t/exe
elif [ "$(uname -m)" = aarch64 ]; then
  # The -mcmodel=large option is incompatible with -fPIC on aarch64, see
  # https://gcc.gnu.org/onlinedocs/gcc/AArch64-Options.html#index-mcmodel_003dlarge
  $CXX -B. -o $t/exe $t/a.cc -mcmodel=large -fno-PIC
  $t/exe

  $CXX -B. -o $t/exe $t/a.cc -static -mcmodel=large -fno-PIC
  $t/exe
fi

echo OK
