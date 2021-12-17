#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -xc -
void foo() {}
EOF

clang -fuse-ld=$mold -shared -o $t/libfoo.so $t/a.o
ar crs $t/libfoo.a $t/a.o

cat <<EOF | clang -c -o $t/b.o -xc -
void foo();
int main() {
  foo();
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/b.o -Wl,--as-needed \
  $t/libfoo.so $t/libfoo.a
ldd $t/exe | grep -q libfoo

clang -fuse-ld=$mold -o $t/exe $t/b.o -Wl,--as-needed \
  $t/libfoo.a $t/libfoo.so
! ldd $t/exe | grep -q libfoo || false

echo OK
