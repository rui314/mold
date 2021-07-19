#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -Wl,-build-id=sha1 -fuse-ld=$mold $t/a.o -o - > $t/exe
chmod 755 $t/exe
$t/exe | grep -q 'Hello world'

echo OK
