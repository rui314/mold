#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

echo 'int main() {}' | cc -o $t/exe -xc -
$mold -dump $t/exe > $t/log

grep -q 'magic: 0xfeedfacf' $t/log
grep -q 'segname: __PAGEZERO' $t/log

echo OK
