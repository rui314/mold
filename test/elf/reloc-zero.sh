#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
foo: jmp 0
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
int main() {}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o

echo OK
