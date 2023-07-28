#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
extern char *msg;
void hello() { printf("%s\n", msg); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
char *msg = "Hello world";
void hello();
int main() { hello(); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'Hello world'
