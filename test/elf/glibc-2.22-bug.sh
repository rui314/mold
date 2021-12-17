#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

# glibc 2.22 or prior have a bug that ld-linux.so.2 crashes on dlopen()
# if .rela.dyn and .rela.plt are not contiguous in a given DSO.
# This test verifies that these sections are contiguous in mold's output.

cat <<EOF | cc -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld=$mold -o $t/b.so -shared $t/a.o
readelf -W --sections $t/b.so | fgrep -A1 .rela.dyn | fgrep -q .rela.plt

echo OK
