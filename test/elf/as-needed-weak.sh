#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -o $t/a.o -c -xc -
__attribute__((weak)) int fn1();

int main() {
  fn1();
}
EOF

cat <<EOF | cc -o $t/b.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int fn1() { return 42; }
EOF

cat <<EOF | cc -o $t/c.so -shared -fPIC -Wl,-soname,libbar.so -xc -
int fn2() { return 42; }
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.so $t/c.so

readelf --dynamic $t/exe > $t/readelf
fgrep -q 'Shared library: [libfoo.so]' $t/readelf
fgrep -q 'Shared library: [libbar.so]' $t/readelf

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-as-needed $t/b.so $t/c.so

readelf --dynamic $t/exe > $t/readelf
! fgrep -q 'Shared library: [libfoo.so]' $t/readelf || false
! fgrep -q 'Shared library: [libbar.so]' $t/readelf || false

echo OK
