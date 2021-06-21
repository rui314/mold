#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

rm -f $t/b.script
touch $t/b.script

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,--version-script,$t/b.script
$t/exe | grep -q 'Hello world'

echo OK
