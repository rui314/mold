#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/libfoo.dylib -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | cc -o $t/a.o -c -xc -
int main() {}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -L$t -Wl,-lfoo
otool -l $t/exe | grep -A3 LOAD_DY | grep -q libfoo.dylib

clang -fuse-ld=$mold -o $t/exe $t/a.o -L$t -Wl,-lfoo -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 LOAD_DY > $t/log
! grep -q libfoo.dylib $t/log || false

echo OK
