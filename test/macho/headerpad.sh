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

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-headerpad,0
otool -l $t/exe | grep -A5 'sectname __text' | grep -q 'addr 0x00000001000006f8'

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-headerpad,0x10000
otool -l $t/exe | grep -A5 'sectname __text' | grep -q 'addr 0x00000001000106f8'

echo OK
