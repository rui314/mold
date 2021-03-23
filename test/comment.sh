#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
int main() {}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
readelf -p .comment $t/exe | grep -q 'mold'

echo OK
