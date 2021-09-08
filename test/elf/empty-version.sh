#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -c -o $t/a.o -xc -
void foo1() {}
void foo2() {}

__asm__(".symver foo1, bar1@");
__asm__(".symver foo2, bar2@@");
EOF

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o

readelf --dyn-syms $t/b.so | grep -q 'bar1$'
readelf --dyn-syms $t/b.so | grep -q 'bar2$'

echo OK
