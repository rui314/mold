#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { foo(); }
EOF

not ./mold -o $t/exe $t/a.o --color-diagnostics 2>&1 | not grep -q $'\033'
not ./mold -o $t/exe $t/a.o --color-diagnostics=always 2>&1 | grep -q $'\033'
not ./mold -o $t/exe $t/a.o --color-diagnostics=never 2>&1 | not grep -q $'\033'
not ./mold -o $t/exe $t/a.o --color-diagnostics=auto 2>&1 | not grep -q $'\033'
