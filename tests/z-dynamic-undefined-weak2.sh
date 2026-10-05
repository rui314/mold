#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIE
#include <stdio.h>
__attribute__((weak)) extern int foo;
int main() { printf("%lu\n", (unsigned long)&foo); }
EOF

if $CC -B. -o $t/exe $t/a.o -no-pie -Wl,-z,dynamic-undefined-weak >& $t/log; then
  $QEMU $t/exe | grep -x 0
else
  grep 'recompile with -fPIE or -fPIC' $t/log
fi
