#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -xc -
void foo() {}
EOF

clang -fuse-ld=$mold -o $t/b.so -shared $t/a.o -Wl,-soname,foo
readelf --dynamic $t/b.so | fgrep -q 'Library soname: [foo]'

echo OK
