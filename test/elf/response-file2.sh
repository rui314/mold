#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo();
int main() { foo(); }
EOF

cat <<EOF | $CC -c -o $t/'b c.o' -xc -
void foo() {}
EOF

echo "'$t'/b\ c.o" > $t/rsp
$CC -o $t/exe $t/a.o -Wl,@$t/rsp
