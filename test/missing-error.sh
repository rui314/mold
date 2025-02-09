#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();

int main() {
  foo();
}
EOF

not ./mold -o $t/exe $t/a.o 2> $t/log

grep 'undefined symbol: foo' $t/log
grep '>>> .*a\.o' $t/log
