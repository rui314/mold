#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void say() {
  printf("Hello\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
void say() {
  printf("Howdy\n");
}
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
void say();
int main() {
  say();
}
EOF

mkdir -p $t/x $t/y

ar rcs $t/x/libfoo.a $t/a.o
$CC -shared -o $t/y/libfoo.dylib $t/b.o

$CC --ld-path=$mold -o $t/exe $t/c.o -Wl,-L$t/x -Wl,-L$t/y -lfoo
$t/exe | grep -q Hello

$CC --ld-path=$mold -o $t/exe $t/c.o -Wl,-L$t/x -Wl,-L$t/y -lfoo \
 -Wl,-search_paths_first
$t/exe | grep -q Hello
