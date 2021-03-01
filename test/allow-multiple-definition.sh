#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

echo 'int main() { return 0; }' > $t/a.c
echo 'int main() { return 0; }' > $t/b.c

! clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.c $t/b.c 2> /dev/null
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.c $t/b.c -Wl,-allow-multiple-definition

echo OK
