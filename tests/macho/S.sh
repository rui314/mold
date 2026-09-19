#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF > $t/a.c
#include <stdio.h>
void hello() { printf("Hello world\n"); }
int main(){ hello(); }
EOF

$CC -o $t/a.o -c -g $t/a.c

$CC --ld-path=$mold -o $t/exe1 $t/a.o -g
nm -pa $t/exe1 | grep -qw OSO

$CC --ld-path=$mold -o $t/exe2 $t/a.o -g -Wl,-S
nm -pa $t/exe2 > $t/log2
! grep -qw OSO $t/log2 || false
