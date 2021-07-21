#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -flto -c -o $t/a.o -xc -
int main() {}
EOF

! clang -fuse-ld=$mold -o $t/exe $t/a.o &> $t/log
grep -q '.*/a.o: .*mold does not support LTO' $t/log

echo OK
