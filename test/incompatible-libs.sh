#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -m32 -c -o $t/a.o -xc -
char hello[] = "Hello world";
EOF

mkdir -p $t/lib32
ar crs $t/lib32/libfoo.a $t/a.o
clang -m32 -shared -o $t/lib32/libfoo.so $t/a.o

cat <<EOF | cc -c -o $t/d.o -xc -
char hello[] = "Hello world";
EOF

mkdir -p $t/lib64
ar crs $t/lib64/libfoo.a $t/d.o
clang -shared -o $t/lib64/libfoo.so $t/d.o

cat <<EOF | cc -c -o $t/e.o -xc -
#include <stdio.h>

extern char hello[];

int main() {
  printf("%s\n", hello);
}
EOF

mkdir -p $t/script
echo 'OUTPUT_FORMAT(elf32-i386)' > $t/script/libfoo.so

clang -fuse-ld=`pwd`/../mold -o $t/exe -L$t/script -L$t/lib32 -L$t/lib64 \
  $t/e.o -lfoo -rpath $t/lib64 >& $t/log

grep -q 'script/libfoo.so: skipping incompatible file' $t/log
grep -q 'lib32/libfoo.so: skipping incompatible file' $t/log
grep -q 'lib32/libfoo.a: skipping incompatible file' $t/log
$t/exe | grep -q 'Hello world'

echo OK
