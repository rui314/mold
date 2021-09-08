#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

echo 'int main() { return 0; }' > $t/a.c
echo 'int main() { return 0; }' > $t/b.c

! clang -fuse-ld=$mold -o $t/exe $t/a.c $t/b.c 2> /dev/null || false
clang -fuse-ld=$mold -o $t/exe $t/a.c $t/b.c -Wl,-allow-multiple-definition
clang -fuse-ld=$mold -o $t/exe $t/a.c $t/b.c -Wl,-z,muldefs

echo OK
