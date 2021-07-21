#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

ldd $mold-wrapper.so | grep -q libasan && { echo skipped; exit; }

rm -rf $t
mkdir -p $t/bin $t/lib/mold
cp $mold $t/bin

$t/bin/mold -run true 2>&1 | grep -q 'mold-wrapper.so is missing'

rm -rf $t
mkdir -p $t/bin $t/lib/mold
cp $mold $t/bin
cp $mold-wrapper.so $t/bin

$t/bin/mold -run bash -c 'echo $LD_PRELOAD' | grep -q '/bin/mold-wrapper.so'

rm -rf $t
mkdir -p $t/bin $t/lib/mold
cp $mold $t/bin
cp $mold-wrapper.so $t/lib/mold

$t/bin/mold -run bash -c 'echo $LD_PRELOAD' | grep -q '/lib/mold/mold-wrapper.so'

rm -rf $t
mkdir -p $t/bin $t/lib/mold
cp $mold $t/bin
cp $mold-wrapper.so $t/bin
cp $mold-wrapper.so $t/lib/mold

$t/bin/mold -run bash -c 'echo $LD_PRELOAD' | grep -q '/bin/mold-wrapper.so'

echo OK
