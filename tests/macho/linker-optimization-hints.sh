#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -O2
#include <stdio.h>

char x1 = -1;
short x2 = -1;
int x3 = -1;
long x4 = -1;
int x5[] = {0, 1, 2, 3};
long x6[] = {0, 1, 2, 3};

void hello() {
  printf("Hello world ");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -O2
#include <stdio.h>

void hello();

extern char x1;
extern short x2;
extern int x3;
extern long x4;
extern int x5[];
extern long x6[];

int main() {
  hello();
  printf("%d %d %d %ld %d %ld\n", x1, x2, x3, x4, x5[2], x6[3]);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world -1 -1 -1 -1 2 3'
