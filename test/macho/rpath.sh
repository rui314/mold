#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int main() {}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-rpath,foo -Wl,-rpath,@bar
otool -l $t/exe > $t/log

grep -A3 'cmd LC_RPATH' $t/log | grep -q 'path foo'
grep -A3 'cmd LC_RPATH' $t/log | grep -q 'path @bar'

echo OK
