#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/exe -xc - -Wl,-adhoc_codesign
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$t/exe | fgrep -q 'Hello world'

echo OK
