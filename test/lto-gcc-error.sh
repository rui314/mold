#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | gcc -flto -c -o $t/a.o -xc -
int main() {}
EOF

! clang -fuse-ld=$mold -o $t/exe $t/a.o &> $t/log
grep -q '.*/a.o: .*mold does not support LTO' $t/log

echo OK
