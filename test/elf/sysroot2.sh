#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

mkdir -p $t/sysroot/foo

cat <<EOF > $t/a.script
INPUT(=/foo/x.o)
EOF

cat <<EOF > $t/sysroot/b.script
INPUT(/foo/y.o)
EOF

cat <<EOF | clang -c -o $t/sysroot/foo/x.o -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | clang -c -o $t/sysroot/foo/y.o -xc -
#include <stdio.h>
void hello2() {
  printf("Hello world\n");
}
EOF

cat <<EOF | clang -c -o $t/c.o -xc -
void hello();
void hello2();

int main() {
  hello();
  hello2();
}
EOF

clang -fuse-ld=$mold -o $t/exe -Wl,--sysroot=$t/sysroot \
  $t/a.script $t/sysroot/b.script $t/c.o

echo OK
