#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/long-long-long-filename.o -c -xc -
int three() { return 3; }
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
int five() { return 5; }
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

int three();
int five();

int main() {
  printf("%d\n", three() + five());
}
EOF

rm -f $t/d.a
(cd $t; ar rcs d.a long-long-long-filename.o b.o)

clang -fuse-ld=$mold -Wl,--trace -o $t/exe $t/c.o $t/d.a > $t/log

fgrep -q 'static-archive/d.a(long-long-long-filename.o)' $t/log
fgrep -q 'static-archive/d.a(b.o)' $t/log
fgrep -q static-archive/c.o $t/log

$t/exe | grep -q '8'

echo OK
