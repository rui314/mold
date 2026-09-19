#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int x = 42;
int *p = &x;
char msg[] = "Hello";
char *pmsg = msg;

int main() {
  printf("%d %s\n", *p, pmsg);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '42 Hello'
