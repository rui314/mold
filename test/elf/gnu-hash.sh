#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | c++ -o $t/exe -Wl,-hash-style=gnu -xc++ -
#include <iostream>

int main() {
  std::cout << "foo\n";
}
EOF

$t/exe | grep -q foo

echo OK
