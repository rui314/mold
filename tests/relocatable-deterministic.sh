#!/usr/bin/env bash
. $(dirname $0)/common.inc

test_cflags -g3 || skip

# With -g3, GCC puts .debug_macro contents in COMDAT groups of the same
# section name. A relocatable output keeps each group member in an output
# section of its own, and the order of those sections must not depend on
# thread scheduling.
cat <<EOF | $CC -c -o $t/a.o -xc - -g3
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc - -g3
#include <stdlib.h>
void hello();
int main() { hello(); }
EOF

./mold -r -o $t/c1.o $t/a.o $t/b.o
./mold -r -o $t/c2.o $t/a.o $t/b.o
./mold -r -o $t/c3.o $t/a.o $t/b.o
./mold -r -o $t/c4.o $t/a.o $t/b.o
./mold -r -o $t/c5.o $t/a.o $t/b.o

cmp $t/c1.o $t/c2.o
cmp $t/c1.o $t/c3.o
cmp $t/c1.o $t/c4.o
cmp $t/c1.o $t/c5.o
