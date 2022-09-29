#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

ldd mold-wrapper.so | grep -q libasan && { echo skipped; exit; }

nm mold | grep -q '__[at]san_init' && { echo skipped; exit; }

rm -rf $t
mkdir -p $t/bin $t/lib/mold
cp mold $t/bin
cp mold-wrapper.so $t/bin

$t/bin/mold -run bash -c 'echo $LD_PRELOAD' | grep -q '/bin/mold-wrapper.so'

echo OK
