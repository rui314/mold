#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
void foo() {}
EOF

clang -fuse-ld=$mold -shared -o $t/libfoo.so $t/a.o -Wl,--soname,libfoo
clang -fuse-ld=$mold -shared -o $t/libbar.so $t/a.o

cat <<EOF | clang -c -o $t/b.o -xc -
int main() {}
EOF

clang -fuse-ld=$mold -o $t/exe $t/b.o $t/libfoo.so
readelf --dynamic $t/exe | fgrep -q 'Shared library: [libfoo]'

clang -fuse-ld=$mold -o $t/exe $t/b.o -L $t -lfoo
readelf --dynamic $t/exe | fgrep -q 'Shared library: [libfoo]'

clang -fuse-ld=$mold -o $t/exe $t/b.o $t/libbar.so
readelf --dynamic $t/exe | grep -Pq 'Shared library: \[.*dt-needed/libbar\.so\]'

clang -fuse-ld=$mold -o $t/exe $t/b.o -L$t -lbar
readelf --dynamic $t/exe | fgrep -q 'Shared library: [libbar.so]'

echo OK
