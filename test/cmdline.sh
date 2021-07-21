#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

(! $mold -zfoo) 2>&1 | grep -q 'unknown command line option: -zfoo'
(! $mold -z foo) 2>&1 | grep -q 'unknown command line option: -z foo'
(! $mold -abcdefg) 2>&1 | grep -q 'unknown command line option: -abcdefg'
(! $mold --abcdefg) 2>&1 | grep -q 'unknown command line option: --abcdefg'

echo OK
