#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The input is large enough to be memory-mapped, and the output is much
# smaller because --gc-sections drops the array. The output must not be
# written over the input while the input is being read.
cat <<EOF | $CC -c -fdata-sections -o $t/a.o -xc -
char big[100000] = {1};
void _start() {}
EOF

./mold --gc-sections -o $t/exe $t/a.o

cp $t/a.o $t/b.o
./mold --gc-sections -o $t/b.o $t/b.o
cmp $t/exe $t/b.o

cp $t/a.o $t/c.o
rm -f $t/c.a
ar rc $t/c.a $t/c.o
./mold --gc-sections -o $t/c.a --whole-archive $t/c.a
cmp $t/exe $t/c.a

cp $t/a.o $t/d.o
rm -f $t/d.a
ar rcT $t/d.a $t/d.o
./mold --gc-sections -o $t/d.o --whole-archive $t/d.a
cmp $t/exe $t/d.o
