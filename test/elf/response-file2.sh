#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo();
int main() { foo(); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo() {}
EOF

echo "'$t/b.o'" > $t/rsp1
echo "@$t/rsp1" > $t/rsp2

$CC -B. -o $t/exe $t/a.o -Wl,@$t/rsp2
