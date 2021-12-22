#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

$mold -z no-such-opt 2>&1 | grep -q 'unknown command line option: -z no-such-opt'
$mold -zno-such-opt 2>&1 | grep -q 'unknown command line option: -zno-such-opt'

echo OK
