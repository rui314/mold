#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo();
void bar();
int main() { foo(); bar(); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo() {}
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
void bar() {}
EOF

echo "'$t/b.o' '$t/c.o'" > $t/rsp
./mold -o $t/d.so -shared $t/a.o @$t/rsp
