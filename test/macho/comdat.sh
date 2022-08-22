#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc++ -
#include <iostream>
struct T {
  T() { std::cout << "foo "; }
};
T x;
EOF

cat <<EOF | cc -o $t/b.o -c -xc++ -
#include <iostream>
struct T {
  T() { std::cout << "foo "; }
};
T y;
EOF

cat <<EOF | cc -o $t/c.o -c -xc++ -
#include <iostream>
int main() {
  std::cout << "bar\n";
}
EOF

c++ --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^foo foo bar$'

echo OK
