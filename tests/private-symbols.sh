#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
int main() { hello(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
objdump --macho --syms $t/exe > $t/log
! grep ' ltmp' $t/log || false
