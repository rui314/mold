#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main();

void print() {
  printf("%d\n", (char *)print < (char *)main);
}

int main() {
  print();
}
EOF

cat <<EOF > $t/order1
_print
_main
EOF

cat <<EOF > $t/order2
_main
_print
EOF

cc --ld-path=./ld64 -o $t/exe1 $t/a.o -Wl,-order_file,$t/order1
$t/exe1 | grep -q '^1$'

cc --ld-path=./ld64 -o $t/exe2 $t/a.o -Wl,-order_file,$t/order2
$t/exe2 | grep -q '^0$'

echo OK
