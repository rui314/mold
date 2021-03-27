#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

../mold -v | grep -Pq 'mold .*\(compatible with GNU ld\)'
../mold --version | grep -Pq 'mold .*\(compatible with GNU ld\)'

echo OK
