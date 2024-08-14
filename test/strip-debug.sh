#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -g
static void foo() {}
void bar() {}
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--strip-debug

readelf -W --sections $t/exe > $t/log
! grep -Fq .debug_info $t/log || false

readelf -W --symbols $t/exe | grep -q ' bar'
