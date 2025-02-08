#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() {}' | $CC -m32 -o $t/exe -xc - >& /dev/null || skip

nm mold | grep '__tsan_init' && skip

cat <<EOF | $CC -m32 -c -o $t/a.o -xc -
char hello[] = "Hello world";
EOF

mkdir -p $t/lib32
$CC -m32 -shared -o $t/lib32/libfoo.so $t/a.o

cat <<EOF | $CC -c -o $t/d.o -xc -
char hello[] = "Hello world";
EOF

mkdir -p $t/lib64
$CC -shared -o $t/lib64/libfoo.so $t/d.o

cat <<EOF | $CC -c -o $t/e.o -xc -
#include <stdio.h>

extern char hello[];

int main() {
  printf("%s\n", hello);
}
EOF

mkdir -p $t/script
echo 'GROUP(libfoo.so)' > $t/script/libfoo.so

$CC -B. -o $t/exe -L$t/lib32 -L$t/lib64 -lfoo $t/e.o -Wl,-rpath $t/lib64 |&
  grep 'lib32/libfoo.so: skipping incompatible file'

$QEMU $t/exe | grep 'Hello world'
