#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

! clang -fuse-ld=$mold -shared -o $t/b.dylib $t/a.o -Wl,-pagezero_size,0x1000 >& $t/log
fgrep -q ' -pagezero_size option can only be used when linking a main executable' $t/log

echo OK
