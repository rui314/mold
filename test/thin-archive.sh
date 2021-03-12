#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
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
(cd $t; ar rcsT d.a long-long-long-filename.o b.o)

clang -fuse-ld=`pwd`/../mold -Wl,--trace -o $t/exe $t/c.o $t/d.a > $t/log

grep -Pq 'thin-archive/d.a\(.*long-long-long-filename.o\)' $t/log
grep -Pq 'thin-archive/d.a\(.*b.o\)' $t/log
fgrep -q thin-archive/c.o $t/log

$t/exe | grep -q '8'

echo OK
