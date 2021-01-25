#!/bin/bash
set -e
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

../mold --trace -o $t/exe /usr/lib/x86_64-linux-gnu/crt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
  $t/c.o $t/d.a \
  /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
  /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
  /lib/x86_64-linux-gnu/libc.so.6 \
  /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
  /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2 \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
  /usr/lib/x86_64-linux-gnu/crtn.o > $t/log

grep -Pq 'thin-archive/d.a\(.*long-long-long-filename.o\)' $t/log
grep -Pq 'thin-archive/d.a\(.*b.o\)' $t/log
fgrep -q thin-archive/c.o $t/log

$t/exe | grep -q '8'

echo OK
