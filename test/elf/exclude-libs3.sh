#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -xc -c -o $t/a.o -
void foo();
void bar() { foo(); }
EOF

rm -f $t/b.a
ar crs $t/b.a $t/a.o

cat <<EOF | clang -xc -c -o $t/c.o -
void bar();
void foo() { bar(); }
EOF

clang -fuse-ld=$mold -shared -o $t/d.so $t/c.o $t/b.a -Wl,-exclude-libs=ALL
readelf --dyn-syms $t/d.so > $t/log
fgrep -q foo $t/log

echo OK
