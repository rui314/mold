#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -xc -o $t/a.o -
int main() {}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_RELRO ' $t/log

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-z,norelro
readelf --segments -W $t/exe > $t/log
if grep -q 'GNU_RELRO ' $t/log; then false; fi

echo OK
