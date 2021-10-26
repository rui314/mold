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

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-platform_version,macos,13.5,12.0

otool -l $t/exe > $t/log
fgrep -q 'minos 13.5' $t/log
fgrep -q 'sdk 12.0' $t/log

echo OK
