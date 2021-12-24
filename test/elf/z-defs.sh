#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -xc -
void foo();
void bar() { foo(); }
EOF

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o
clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o -Wl,-z,nodefs

! clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o -Wl,-z,defs \
  2> $t/log || false
grep -q 'undefined symbol:.* foo' $t/log

! clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o -Wl,-no-undefined \
  2> $t/log || false
grep -q 'undefined symbol:.* foo' $t/log

clang -fuse-ld=$mold -shared -o $t/c.so $t/a.o -Wl,-z,defs \
  -Wl,--warn-unresolved-symbols 2> $t/log
grep -q 'undefined symbol: .* foo$' $t/log
readelf --dyn-syms $t/c.so | grep -q ' foo$'

echo OK
