#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

echo ' -help' > $t/rsp
./ld64 @$t/rsp | grep -q Usage

echo OK
