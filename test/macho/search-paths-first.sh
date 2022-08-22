#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
void say() {
  printf("Hello\n");
}
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>
void say() {
  printf("Howdy\n");
}
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
void say();
int main() {
  say();
}
EOF

mkdir -p $t/x $t/y

ar rcs $t/x/libfoo.a $t/a.o
cc -shared -o $t/y/libfoo.dylib $t/b.o

cc --ld-path=./ld64 -o $t/exe $t/c.o -Wl,-L$t/x -Wl,-L$t/y -lfoo
$t/exe | grep -q Hello

cc --ld-path=./ld64 -o $t/exe $t/c.o -Wl,-L$t/x -Wl,-L$t/y -lfoo \
 -Wl,-search_paths_first
$t/exe | grep -q Hello

echo OK
