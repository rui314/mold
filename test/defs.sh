#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -xc -
void foo();
void bar() { foo(); }
EOF

clang -fuse-ld=`pwd`/../mold -shared -o $t/b.so $t/a.o
clang -fuse-ld=`pwd`/../mold -shared -o $t/b.so $t/a.o -Wl,-z,nodefs

! clang -fuse-ld=`pwd`/../mold -shared -o $t/b.so $t/a.o -Wl,-z,defs 2> $t/log || false
grep -q 'undefined symbol:.* foo' $t/log

! clang -fuse-ld=`pwd`/../mold -shared -o $t/b.so $t/a.o -Wl,-no-undefined 2> $t/log || false
grep -q 'undefined symbol:.* foo' $t/log

echo OK
