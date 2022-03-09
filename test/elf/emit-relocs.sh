#!/bin/bash
export LC_ALL=C
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | cc -o $t/a.o -c -fPIC -xc -
#include <stdio.h>
int main() {
  puts("Hello world");
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-emit-relocs
$t/exe | grep -q 'Hello world'

readelf -r $t/exe | grep -q 'R_X86_64_PLT32.* puts - 4'

echo OK
