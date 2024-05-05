#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = ppc64 ] && skip

cat <<EOF | $CC -c -o $t/a.o -xc - -ffunction-sections
 __attribute__((retain)) int foo() {}
int bar() {}
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-gc-sections
nm $t/exe > $t/log
grep -q foo $t/log
! grep -q bar $t/log || false
