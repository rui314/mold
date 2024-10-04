#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl foo
.data
foo:
.word bar(TARGET1)
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
#include <stdio.h>
extern char *foo;
char bar[] = "Hello world";
int main() { printf("%s\n", foo); }
EOF

$CC -B. -o $t/exe -pie $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'Hello world'
