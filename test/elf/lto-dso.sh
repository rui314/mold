#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() {}' | $CC -flto -o /dev/null -xc - >& /dev/null \
  || skip

cat <<EOF | $CC -flto -c -fPIC -o $t/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/b.so -flto $t/a.o
nm -D $t/b.so | grep -q 'T foo'
