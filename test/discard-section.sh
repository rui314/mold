#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".foo"))) int foo;
__attribute__((section(".bar"))) int bar;
int main() {}
EOF

$CC -B. -o $t/exe0 $t/a.o
readelf -SW $t/exe0 | grep -F .foo
readelf -SW $t/exe0 | grep -F .bar

$CC -B. -o $t/exe1 $t/a.o -Wl,--discard-section=.foo
readelf -SW $t/exe1 | not grep -F .foo
readelf -SW $t/exe1 | grep -F .bar

$CC -B. -o $t/exe2 $t/a.o -Wl,--discard-section=.foo,--discard-section=.bar,--no-discard-section=.foo
readelf -SW $t/exe2 | grep -F .foo
readelf -SW $t/exe2 | not grep -F .bar
