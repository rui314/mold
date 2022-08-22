#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | c++ -o $t/a.o -c -xc++ - -flto
#include <iostream>
int main() {
  std::cout << "Hello world\n";
}
EOF

c++ --ld-path=./ld64 -o $t/exe $t/a.o -flto -dead_strip_dylibs
$t/exe | grep -q 'Hello world'

echo OK
