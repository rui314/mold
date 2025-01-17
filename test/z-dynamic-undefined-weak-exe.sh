#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
__attribute__((weak)) void fn();
int main() { fn(); }
EOF

$CC -B. -o $t/exe1 $t/a.o -pie
! readelf -W --dyn-syms $t/exe1 | grep -q ' fn$' || false

$CC -B. -o $t/exe2 $t/a.o -pie -Wl,-z,dynamic-undefined-weak
readelf -W --dyn-syms $t/exe2 | grep -q ' fn$'
