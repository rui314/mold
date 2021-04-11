#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fcommon -c -xc -o $t/a.o -
int foo;
EOF

cat <<EOF | clang -fcommon -c -xc -o $t/b.o -
int foo;

int main() {
  return 0;
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o > $t/log
! fgrep -q 'multiple common symbols' $t/log || false

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o -Wl,-warn-common 2> $t/log
fgrep -q 'multiple common symbols' $t/log

echo OK
