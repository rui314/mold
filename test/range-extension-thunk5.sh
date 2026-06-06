#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = sh4 ] && skip

# Regression test for https://github.com/rui314/mold/issues/1538.
#
# On targets that use range extension thunks (ARM and PPC), mold's first
# thunk-creation pass pessimistically routes every PLT-resolved call
# through a thunk. If a single batch of input sections references a huge
# number of distinct imported functions, that first-pass thunk group can
# grow past max_thunk_size and trip an assertion in
# create_range_extension_thunks(). 50,000 imports exceed the historical
# 1 MiB cap on AArch64 (32 bytes per thunk entry), so this exercises the
# large-thunk-group path and checks that every call reaches its target.
n=50000

seq 1 $n | sed 's/.*/int func&(void) { return &; }/' > $t/a.c
$CC -B. -o $t/b.so -shared $t/a.c

echo '#include <stdio.h>' > $t/b.c
seq 1 $n | sed 's/.*/int func&(void);/' >> $t/b.c
echo 'int main() {' >> $t/b.c
echo '  int sum = 0;' >> $t/b.c
seq 1 $n | sed 's/.*/  sum += func&();/' >> $t/b.c
echo '  printf("%d\n", sum);' >> $t/b.c
echo '}' >> $t/b.c

$CC -c -o $t/c.o $t/b.c
$CC -B. -o $t/exe $t/c.o $t/b.so

sum=$((n * (n + 1) / 2))
$QEMU $t/exe | grep -E "^$sum\$"
