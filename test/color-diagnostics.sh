#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { foo(); }
EOF

not ./mold -o $t/exe $t/a.o --color-diagnostics |& not grep -q $'\033'
not ./mold -o $t/exe $t/a.o --color-diagnostics=always |& grep -q $'\033'
not ./mold -o $t/exe $t/a.o --color-diagnostics=never |& not grep -q $'\033'
not ./mold -o $t/exe $t/a.o --color-diagnostics=auto |& not grep -q $'\033'
