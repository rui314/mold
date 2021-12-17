#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t


cat <<EOF | cc -o $t/a.o -c -xc -
int main() {}
EOF

! clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-Z > $t/log 2>&1
grep -q 'library not found: -lSystem' $t/log

echo OK
