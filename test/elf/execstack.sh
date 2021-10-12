#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -xc -o $t/a.o -
int main() {}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-z,execstack
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_STACK.* RWE ' $t/log

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-z,execstack \
  -Wl,-z,noexecstack
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_STACK.* RW ' $t/log

clang -fuse-ld=$mold -o $t/exe $t/a.o
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_STACK.* RW ' $t/log

echo OK
