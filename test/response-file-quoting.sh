#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo();
int main() { foo(); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo() {}
EOF

echo $t/a.o > $t/rsp1
echo $t'"\/b."\o' >> $t/rsp1
./mold -o $t/c.so -shared @$t/rsp1

echo $t/a.o > $t/rsp2
echo '\foo\bar' >> $t/rsp2
not ./mold -o $t/d.so -shared @$t/rsp2 |& grep 'cannot open foobar:'
